// ELK layout for a plan: the `child_of` hierarchy top-down with the layered
// algorithm. Only tree edges shape the layout; blockers and cross-references
// are drawn over it, so the hierarchy stays readable as they come and go.

import { EDGE, type Graph } from "@open-lakehouse/factory-client";
import ELK, { type ElkNode } from "elkjs/lib/elk.bundled.js";
import { useEffect, useRef, useState } from "react";

const elk = new ELK();

/** The node card's box. `WorkNode` renders at exactly this size. */
export const NODE_SIZE = { width: 248, height: 104 } as const;

const OPTIONS: Record<string, string> = {
  "elk.algorithm": "layered",
  "elk.direction": "DOWN",
  "elk.layered.spacing.nodeNodeBetweenLayers": "64",
  "elk.spacing.nodeNode": "32",
  "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
  "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
};

export type Positions = Map<string, { x: number; y: number }>;

export async function layoutGraph(graph: Graph): Promise<Positions> {
  const elkGraph: ElkNode = {
    id: "root",
    layoutOptions: OPTIONS,
    children: graph.nodes.map((n) => ({ id: n.ref, ...NODE_SIZE })),
    edges: graph.edges
      .filter((e) => e.kind === EDGE.childOf)
      .map((e, i) => ({ id: `t${i}`, sources: [e.to], targets: [e.from] })),
  };
  const out = await elk.layout(elkGraph);
  return new Map((out.children ?? []).map((c) => [c.id, { x: c.x ?? 0, y: c.y ?? 0 }]));
}

/**
 * Positions for `graph`, laid out again whenever its shape (nodes or tree
 * edges) changes. Until the first layout resolves it returns an empty map.
 */
export function useLayout(graph: Graph | undefined): Positions {
  const [positions, setPositions] = useState<Positions>(new Map());
  const run = useRef(0);
  const shape = graph
    ? [
        graph.nodes.map((n) => n.ref).join(","),
        graph.edges
          .filter((e) => e.kind === EDGE.childOf)
          .map((e) => `${e.from}>${e.to}`)
          .join(","),
      ].join("|")
    : "";
  // biome-ignore lint/correctness/useExhaustiveDependencies: `shape` is the graph's layout identity.
  useEffect(() => {
    const id = ++run.current;
    if (!graph) return;
    layoutGraph(graph).then((p) => {
      if (id === run.current) setPositions(p);
    });
  }, [shape]);
  return positions;
}
