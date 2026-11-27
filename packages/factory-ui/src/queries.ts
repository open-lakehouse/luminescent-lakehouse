// react-query hooks over factory.v1. Reads are keyed per graph; every write
// invalidates its graph, and `useWatchGraph` invalidates on server-side
// changes (patching a changed run in place first), so views reconcile with
// edits made anywhere else. The watch reconnects when its stream ends, and
// refetches what it may have missed meanwhile.

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
import { useEffect, useState } from "react";
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
  places: ["factory", "places"] as const,
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

/** The places of every capped environment, across graphs: who holds them and
 * who waits, in line. Only fetched while `enabled` (a run on screen waits),
 * and polled then, since runs in other graphs move the line without this
 * graph's watch seeing it. */
export function usePlaces(enabled: boolean, pollMs = 10_000) {
  const client = useFactoryClient();
  return useQuery({
    queryKey: keys.places,
    queryFn: async () => (await client.listPlaces({})).environments,
    enabled,
    refetchInterval: enabled ? pollMs : false,
  });
}

/** Where a watch is: on a stream (or opening its first), or between streams
 * after one ended. */
export type WatchStatus = "live" | "reconnecting";

export interface WatchOptions {
  /** The wait before the first reconnect, doubled after each failed one, up
   * to `maxMs`; reset once a stream delivers a message. */
  backoff?: { initialMs?: number; maxMs?: number };
}

/** The wait before reconnect `attempt` (0-based): exponential, capped, with
 * up to a fifth of jitter so clients behind one proxy don't return in step. */
export function backoffDelay(
  attempt: number,
  { initialMs = 500, maxMs = 30_000 }: NonNullable<WatchOptions["backoff"]> = {},
  random: () => number = Math.random,
): number {
  const base = Math.min(maxMs, initialMs * 2 ** attempt);
  return Math.round(base * (1 - 0.2 * random()));
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const t = setTimeout(resolve, ms);
    signal.addEventListener(
      "abort",
      () => {
        clearTimeout(t);
        resolve();
      },
      { once: true },
    );
  });
}

/** Invalidate the graph's queries whenever the server reports a change.
 *
 * A stream ends when a proxy or the server restarts. The watch then
 * reconnects with backoff, and on the first message of every stream after the
 * first it refetches the graph, its runs, revisions and annotations, and the
 * places, so whatever changed while it was away shows. */
export function useWatchGraph(root: string | undefined, options: WatchOptions = {}): WatchStatus {
  const client = useFactoryClient();
  const qc = useQueryClient();
  // Only a reconnect changes it, so a healthy watch never re-renders its host.
  const [reconnecting, setReconnecting] = useState(false);
  const initialMs = options.backoff?.initialMs;
  const maxMs = options.backoff?.maxMs;
  useEffect(() => {
    if (!root) return;
    setReconnecting(false);
    const abort = new AbortController();
    const refetch = async () => {
      await qc.invalidateQueries({ queryKey: keys.graph(root) });
      await qc.invalidateQueries({ queryKey: keys.graphs });
      await qc.invalidateQueries({ queryKey: keys.places });
    };
    (async () => {
      let last: string | undefined;
      let attempt = 0;
      let streams = 0;
      while (!abort.signal.aborted) {
        let first = true;
        try {
          for await (const change of client.watchGraph({ root }, { signal: abort.signal })) {
            const tag = `${change.revision}:${change.sequence}`;
            if (first) {
              first = false;
              attempt = 0;
              setReconnecting(false);
              // A reconnect: changes made meanwhile never reached this
              // client, and a restarted server may count anew, so the tag
              // can't tell. Refetch everything.
              if (streams > 0) await refetch();
              streams += 1;
            } else if (tag !== last) {
              await qc.invalidateQueries({ queryKey: keys.graph(root) });
              await qc.invalidateQueries({ queryKey: keys.graphs });
            }
            if (change.change.case === "run") {
              // Patch the run in place so the canvas updates before the refetch.
              const run = change.change.value;
              qc.setQueryData<Run[]>(keys.runs(root), (runs) =>
                runs ? [...runs.filter((r) => r.ref !== run.ref), run] : runs,
              );
              await qc.invalidateQueries({ queryKey: keys.places });
            }
            last = tag;
          }
        } catch {
          // The stream failed (or was aborted on unmount): reconnect below.
        }
        if (abort.signal.aborted) break;
        setReconnecting(true);
        await sleep(backoffDelay(attempt, { initialMs, maxMs }), abort.signal);
        attempt += 1;
      }
    })();
    return () => abort.abort();
  }, [client, qc, root, initialMs, maxMs]);
  return reconnecting ? "reconnecting" : "live";
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
