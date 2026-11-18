// react-query hooks over factory.v1. Reads are keyed per graph; every write
// invalidates its graph, and `useWatchGraph` invalidates on server-side
// changes (patching a changed run in place first), so views reconcile with
// edits made anywhere else.

import type { MessageInitShape, Registry } from "@bufbuild/protobuf";
import {
  type Edge,
  type GetSchemaResponse,
  type NodeSpec,
  type NodeSpecSchema,
  type Phase,
  payloadRegistry,
  type Run,
  schemaRegistry,
} from "@open-lakehouse/factory-client";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useFactoryClient } from "./context.js";

export const keys = {
  schema: ["factory", "schema"] as const,
  graphs: ["factory", "graphs"] as const,
  graph: (root: string) => ["factory", "graph", root] as const,
  revisions: (root: string) => ["factory", "graph", root, "revisions"] as const,
  revision: (root: string, hash: string) => ["factory", "graph", root, "revision", hash] as const,
  diff: (root: string, base: string, target: string, compact: boolean) =>
    ["factory", "graph", root, "diff", base, target, compact] as const,
  annotations: (root: string, node: string) =>
    ["factory", "graph", root, "annotations", node] as const,
  runs: (root: string) => ["factory", "graph", root, "runs"] as const,
};

function useSchemaResponse<T>(select: (r: GetSchemaResponse) => T) {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.schema,
    queryFn: () => client.getSchema({}),
    staleTime: Number.POSITIVE_INFINITY,
    select,
  });
}

const selectSchema = (r: GetSchemaResponse) => r.schema;
const selectRegistry = (r: GetSchemaResponse) => schemaRegistry(r.fileDescriptorSet);

/** The server's schema: kinds, edge kinds, gate conditions and task
 * extensions, with their display metadata. */
export function useSchema() {
  return useSchemaResponse(selectSchema);
}

/** A registry with every payload type the server's extensions register,
 * from GetSchema's descriptors; the built-in types until it loads. */
export function usePayloadRegistry(): Registry {
  return useSchemaResponse(selectRegistry).data ?? payloadRegistry;
}

export function useGraphs() {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.graphs,
    queryFn: async () => (await client.listGraphs({})).graphs,
  });
}

export function useGraph(root: string | undefined) {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.graph(root ?? ""),
    queryFn: () => client.getGraph({ root: root ?? "" }),
    enabled: !!root,
  });
}

export function useRevisions(root: string | undefined) {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.revisions(root ?? ""),
    queryFn: async () => (await client.listRevisions({ root: root ?? "" })).revisions,
    enabled: !!root,
  });
}

/** A revision's tree and text; `hash` empty for the current one. */
export function useRevision(root: string | undefined, hash: string | undefined) {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.revision(root ?? "", hash ?? ""),
    queryFn: () => client.getRevision({ root: root ?? "", rootHash: hash ?? "" }),
    enabled: !!root && hash !== undefined,
    // A revision with a hash never changes.
    staleTime: hash ? Number.POSITIVE_INFINITY : 0,
  });
}

export function useDiff(root: string | undefined, base: string, target: string, compact: boolean) {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.diff(root ?? "", base, target, compact),
    queryFn: async () =>
      (await client.diffRevisions({ root: root ?? "", base, target, compact })).entries,
    enabled: !!root && !!target,
  });
}

export function useAnnotations(root: string | undefined, node: string | undefined) {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.annotations(root ?? "", node ?? ""),
    queryFn: async () => (await client.listAnnotations({ node: node ?? "" })).annotations,
    enabled: !!root && !!node,
  });
}

export function useRuns(root: string | undefined) {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.runs(root ?? ""),
    queryFn: async () => (await client.listRuns({ root: root ?? "" })).runs,
    enabled: !!root,
  });
}

/** Invalidate the graph's queries whenever the server reports a change. */
export function useWatchGraph(root: string | undefined) {
  const client = useFactoryClient();
  const qc = useQueryClient();
  useEffect(() => {
    if (!root) return;
    const abort = new AbortController();
    let last: string | undefined;
    (async () => {
      try {
        for await (const change of client.watchGraph({ root }, { signal: abort.signal })) {
          const tag = `${change.revision}:${change.sequence}`;
          if (change.change.case === "run") {
            // Patch the run in place so the canvas updates before the refetch.
            const run = change.change.value;
            qc.setQueryData<Run[]>(keys.runs(root), (runs) =>
              runs ? [...runs.filter((r) => r.ref !== run.ref), run] : runs,
            );
          }
          if (last !== undefined && tag !== last) {
            await qc.invalidateQueries({ queryKey: keys.graph(root) });
            await qc.invalidateQueries({ queryKey: keys.graphs });
          }
          last = tag;
        }
      } catch {
        // Aborted on unmount, or the stream dropped: the views still refetch
        // after their own writes.
      }
    })();
    return () => abort.abort();
  }, [client, qc, root]);
}

/** The write operations on one graph. Each invalidates the graph after. */
export function useGraphMutations(root: string | undefined) {
  const client = useFactoryClient();
  const qc = useQueryClient();
  const settle = async () => {
    if (!root) return;
    await qc.invalidateQueries({ queryKey: keys.graph(root) });
    await qc.invalidateQueries({ queryKey: keys.graphs });
  };
  const onSettled = settle;
  type Ref = Pick<Edge, "kind" | "from" | "to">;
  const createNode = useMutation({
    mutationFn: (a: {
      parent: string;
      kind: string;
      spec: MessageInitShape<typeof NodeSpecSchema>;
    }) => client.createNode({ parent: a.parent, kind: a.kind, spec: a.spec }),
    onSettled,
  });
  const updateNode = useMutation({
    mutationFn: (a: { ref: string; spec: NodeSpec; etag: string }) =>
      client.updateNode({ ref: a.ref, spec: a.spec, etag: a.etag }),
    onSettled,
  });
  const moveNode = useMutation({
    mutationFn: (a: { ref: string; newParent: string }) => client.moveNode(a),
    onSettled,
  });
  const deleteNode = useMutation({
    mutationFn: (a: { ref: string; recursive: boolean }) => client.deleteNode(a),
    onSettled,
  });
  const addEdge = useMutation({ mutationFn: (edge: Ref) => client.addEdge({ edge }), onSettled });
  const removeEdge = useMutation({
    mutationFn: (edge: Ref) => client.removeEdge({ edge }),
    onSettled,
  });
  const setPhase = useMutation({
    mutationFn: (a: { ref: string; phase: Phase }) => client.setPhase(a),
    onSettled,
  });
  const approve = useMutation({
    mutationFn: (rootHash: string) => client.approveRevision({ root: root ?? "", rootHash }),
    onSettled,
  });
  return { createNode, updateNode, moveNode, deleteNode, addEdge, removeEdge, setPhase, approve };
}
