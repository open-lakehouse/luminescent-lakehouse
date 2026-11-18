// The plan as an interactive graph. The `child_of` hierarchy is the layout;
// blockers are dashed arrows from the blocker to what waits on it, and
// network edges (cross-references, provenance) are dotted and can be hidden.
//
// Interactions, each a write through the caller's callbacks:
// - select a node (click, or arrow keys: up to the parent, down to the first
//   child, left/right to siblings);
// - add a child of an allowed kind from a node's `+` menu;
// - drag a node onto another to reparent it;
// - drag from a node's right handle to another's left handle to add an edge
//   of a kind the schema allows between them;
// - select an edge and press Delete or Backspace to remove it.

import type { Registry } from "@bufbuild/protobuf";
import {
  type ChangeKind,
  type DiffEntry,
  EDGE,
  type Edge,
  type EdgeKind,
  type Graph,
  payloadRegistry,
  type Run,
  type Schema,
  Topology,
} from "@open-lakehouse/factory-client";
import { Button, cn } from "@open-lakehouse/ui-kit";
import {
  Background,
  type ColorMode,
  type Connection,
  Controls,
  type Edge as FlowEdge,
  MarkerType,
  MiniMap,
  type NodeTypes,
  type OnBeforeDelete,
  ReactFlow,
  ReactFlowProvider,
  useNodesState,
  useReactFlow,
} from "@xyflow/react";
import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from "react";
import { cardChips } from "../kinds/index.js";
import {
  childKinds,
  displayName,
  drawableEdgeKinds,
  indexGraph,
  isWithin,
  runBadge,
} from "../model.js";
import { useLayout } from "./layout.js";
import { type WorkFlowNode, WorkNode } from "./WorkNode.js";

const nodeTypes: NodeTypes = { work: WorkNode };

export interface PlanCanvasProps {
  graph: Graph | undefined;
  schema: Schema | undefined;
  /** Decodes payloads for the cards' chips (`usePayloadRegistry`); the
   * built-in types by default. */
  registry?: Registry;
  selected?: string;
  onSelect?: (ref: string | undefined) => void;
  /** Each node's latest run, shown on its card. */
  runs?: Map<string, Run>;
  /** Tint nodes by change, for the diff overlay. */
  changes?: Map<string, DiffEntry>;
  /** Show network (unhashed) edges. Default true. */
  showNetwork?: boolean;
  /** No editing: no `+` menus, no drag-to-reparent, no edge drawing. */
  readOnly?: boolean;
  onAddChild?: (parent: string, kind: string) => void;
  onMove?: (ref: string, newParent: string) => void;
  onAddEdge?: (edge: Pick<Edge, "kind" | "from" | "to">) => void;
  onRemoveEdge?: (edge: Pick<Edge, "kind" | "from" | "to">) => void;
  colorMode?: ColorMode;
  className?: string;
}

export function PlanCanvas(props: PlanCanvasProps) {
  return (
    <ReactFlowProvider>
      <Flow {...props} />
    </ReactFlowProvider>
  );
}

const edgeId = (e: Pick<Edge, "kind" | "from" | "to">) => `${e.kind}|${e.from}|${e.to}`;

function parseEdgeId(id: string): Pick<Edge, "kind" | "from" | "to"> | undefined {
  const [kind, from, to] = id.split("|");
  return kind && from && to ? { kind, from, to } : undefined;
}

function Flow({
  graph,
  schema,
  registry = payloadRegistry,
  selected,
  onSelect,
  changes,
  runs,
  showNetwork = true,
  readOnly,
  onAddChild,
  onMove,
  onAddEdge,
  onRemoveEdge,
  colorMode = "system",
  className,
}: PlanCanvasProps) {
  const idx = useMemo(() => indexGraph(graph), [graph]);
  const positions = useLayout(graph);
  const flow = useReactFlow<WorkFlowNode>();
  const [dropTarget, setDropTarget] = useState<string>();
  const [pending, setPending] = useState<{ from: string; to: string; kinds: EdgeKind[] }>();

  const topology = useMemo(
    () => new Map((schema?.edgeKinds ?? []).map((k) => [k.name, k.topology])),
    [schema],
  );

  const laidOut = useMemo<WorkFlowNode[]>(() => {
    const titleOf = (ref: string) => idx.nodes.get(ref)?.spec?.title;
    return (graph?.nodes ?? []).map((n) => ({
      id: n.ref,
      type: "work",
      position: positions.get(n.ref) ?? { x: 0, y: 0 },
      selected: n.ref === selected,
      data: {
        node: n,
        kindName: displayName(schema, n.kind),
        chips: cardChips(n, { schema, registry, titleOf }),
        childKinds: childKinds(schema, n.kind).map((k) => ({
          kind: k,
          name: displayName(schema, k),
        })),
        change: changes?.get(n.ref)?.change as ChangeKind | undefined,
        run: runBadge(runs?.get(n.ref)),
        dropTarget: n.ref === dropTarget,
        readOnly,
        onAddChild: (kind: string) => onAddChild?.(n.ref, kind),
      },
      draggable: !readOnly && n.ref !== graph?.root,
    }));
  }, [
    graph,
    idx,
    positions,
    schema,
    registry,
    selected,
    changes,
    runs,
    dropTarget,
    readOnly,
    onAddChild,
  ]);
  const [nodes, setNodes, onNodesChange] = useNodesState<WorkFlowNode>(laidOut);
  useEffect(() => setNodes(laidOut), [laidOut, setNodes]);

  // Fit the view once per plan, when its first layout lands (`fitView` on
  // mount would fit the unpositioned nodes).
  const [fitted, setFitted] = useState<string>();
  useEffect(() => {
    if (!graph || positions.size === 0 || fitted === graph.root) return;
    const id = requestAnimationFrame(() => {
      flow.fitView({ padding: 0.15 });
      setFitted(graph.root);
    });
    return () => cancelAnimationFrame(id);
  }, [graph, positions, fitted, flow]);

  const edges = useMemo<FlowEdge[]>(() => {
    const out: FlowEdge[] = [];
    for (const e of graph?.edges ?? []) {
      const t = topology.get(e.kind);
      if (e.kind === EDGE.childOf) {
        out.push({
          id: edgeId(e),
          source: e.to,
          target: e.from,
          sourceHandle: "child",
          targetHandle: "parent",
          type: "smoothstep",
          deletable: false,
          selectable: false,
          style: { stroke: "var(--color-muted-foreground, #888)", strokeOpacity: 0.5 },
        });
      } else if (t === Topology.DAG) {
        // Drawn from the blocker to what waits on it.
        out.push({
          id: edgeId(e),
          source: e.to,
          target: e.from,
          sourceHandle: "out",
          targetHandle: "in",
          label: e.kind === EDGE.blockedBy ? "blocks" : e.kind,
          deletable: !readOnly,
          animated: false,
          markerEnd: { type: MarkerType.ArrowClosed, color: "#e11d48" },
          style: { stroke: "#e11d48", strokeDasharray: "6 4" },
          labelStyle: { fontSize: 10 },
        });
      } else if (showNetwork) {
        out.push({
          id: edgeId(e),
          source: e.from,
          target: e.to,
          sourceHandle: "out",
          targetHandle: "in",
          label: e.kind.replace(/_/g, " "),
          deletable: !readOnly,
          markerEnd: { type: MarkerType.Arrow, color: "#64748b" },
          style: { stroke: "#64748b", strokeDasharray: "2 4" },
          labelStyle: { fontSize: 10 },
        });
      }
    }
    return out;
  }, [graph, topology, showNetwork, readOnly]);

  // The node under the one being dragged that could become its parent.
  const parentCandidate = useCallback(
    (dragged: WorkFlowNode): string | undefined => {
      const kind = dragged.data.node.kind;
      return flow
        .getIntersectingNodes(dragged)
        .map((n) => n.id)
        .find(
          (id) =>
            !isWithin(idx, id, dragged.id) &&
            childKinds(schema, idx.nodes.get(id)?.kind ?? "").includes(kind),
        );
    },
    [flow, idx, schema],
  );

  const onConnect = useCallback(
    (c: Connection) => {
      if (!c.source || !c.target || c.source === c.target) return;
      const from = idx.nodes.get(c.source);
      const to = idx.nodes.get(c.target);
      if (!from || !to) return;
      // Dragged from the source's right handle: the source waits on the
      // target for a blocker, and points at it for a network edge.
      const kinds = drawableEdgeKinds(schema, from.kind, to.kind);
      if (kinds.length === 1) onAddEdge?.({ kind: kinds[0].name, from: from.ref, to: to.ref });
      else if (kinds.length > 1) setPending({ from: from.ref, to: to.ref, kinds });
    },
    [idx, schema, onAddEdge],
  );

  const onBeforeDelete: OnBeforeDelete<WorkFlowNode> = useCallback(
    async ({ edges: doomed }) => {
      for (const e of doomed) {
        const parsed = parseEdgeId(e.id);
        if (parsed && parsed.kind !== EDGE.childOf) onRemoveEdge?.(parsed);
      }
      // The graph changes through the server; nothing is deleted locally.
      return false;
    },
    [onRemoveEdge],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!selected || (e.target as HTMLElement).closest("input,textarea,[contenteditable]"))
        return;
      const parent = idx.parent.get(selected);
      const siblings = parent ? (idx.children.get(parent) ?? []) : [];
      const at = siblings.indexOf(selected);
      let next: string | undefined;
      if (e.key === "ArrowUp") next = parent;
      else if (e.key === "ArrowDown") next = idx.children.get(selected)?.[0];
      else if (e.key === "ArrowLeft") next = siblings[at - 1];
      else if (e.key === "ArrowRight") next = siblings[at + 1];
      else if (e.key === "Escape") return onSelect?.(undefined);
      else return;
      e.preventDefault();
      if (next) {
        onSelect?.(next);
        const n = flow.getNode(next);
        if (n)
          flow.setCenter(n.position.x + 124, n.position.y + 52, {
            duration: 200,
            zoom: flow.getZoom(),
          });
      }
    },
    [selected, idx, onSelect, flow],
  );

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: keyboard navigation for the canvas below.
    <div
      className={cn("relative h-full w-full", className)}
      onKeyDown={onKeyDown}
      data-testid="plan-canvas"
    >
      <ReactFlow<WorkFlowNode>
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodesChange={onNodesChange}
        onNodeClick={(_, n) => onSelect?.(n.id)}
        onPaneClick={() => onSelect?.(undefined)}
        onNodeDrag={(_, n) => setDropTarget(parentCandidate(n))}
        onNodeDragStop={(_, n) => {
          const target = parentCandidate(n);
          setDropTarget(undefined);
          if (target && target !== idx.parent.get(n.id)) onMove?.(n.id, target);
          // Snap back; the next layout places it under its new parent.
          setNodes(laidOut);
        }}
        onConnect={readOnly ? undefined : onConnect}
        nodesConnectable={!readOnly}
        onBeforeDelete={onBeforeDelete}
        deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
        colorMode={colorMode}
        minZoom={0.2}
        proOptions={{ hideAttribution: true }}
      >
        <Background gap={20} />
        <Controls showInteractive={false} />
        <MiniMap pannable zoomable className="!bg-background" />
      </ReactFlow>
      {pending ? (
        <div
          role="dialog"
          aria-label="Choose an edge kind"
          className="absolute top-3 left-1/2 z-10 flex -translate-x-1/2 items-center gap-2 rounded-md border bg-popover p-2 text-sm shadow-md"
        >
          <span className="text-muted-foreground">Add edge:</span>
          {pending.kinds.map((k) => (
            <Button
              key={k.name}
              size="sm"
              variant="secondary"
              onClick={() => {
                onAddEdge?.({ kind: k.name, from: pending.from, to: pending.to });
                setPending(undefined);
              }}
            >
              {k.name.replace(/_/g, " ")}
            </Button>
          ))}
          <Button size="sm" variant="ghost" onClick={() => setPending(undefined)}>
            Cancel
          </Button>
        </div>
      ) : null}
    </div>
  );
}
