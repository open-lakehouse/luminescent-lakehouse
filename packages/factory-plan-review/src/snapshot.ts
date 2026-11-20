// `open_plan_review`'s result, and the factory.v1 client the view runs on.
//
// The view has no network: the tool's `structuredContent` carries a snapshot of
// the RPC answers factory-ui's components ask for (binary factory.v1 messages,
// base64; workgraph-mcp's `review::Snapshot`). An in-memory Connect router
// answers from it, so PlanDiffView and the registry hooks work unchanged, and
// anything else is `unimplemented`.

import { fromBinary } from "@bufbuild/protobuf";
import { base64Decode } from "@bufbuild/protobuf/wire";
import { Code, ConnectError, createRouterTransport } from "@connectrpc/connect";
import {
  createFactoryClient,
  type DiffRevisionsResponse,
  DiffRevisionsResponseSchema,
  type FactoryClient,
  FactoryService,
  type GetGraphResponse,
  GetGraphResponseSchema,
  type GetRevisionResponse,
  GetRevisionResponseSchema,
  type GetSchemaResponse,
  GetSchemaResponseSchema,
} from "@open-lakehouse/factory-client";

/** The snapshot as the tool sends it: base64 of each message. */
export interface WireSnapshot {
  schema: string;
  graph: string;
  target: string;
  /** Empty when no revision was approved yet. */
  base: string;
  diff: string;
  diff_full: string;
}

/** `open_plan_review`'s `structuredContent`. */
export interface PlanReview {
  status: string;
  root: string;
  title: string;
  /** The revision under review. */
  revision: string;
  /** The approved revision the review compares with; `null` if none. */
  approved_revision: string | null;
  /** Compact changes since the approved revision, per kind (`added`, …). */
  changes: Record<string, number>;
  snapshot: WireSnapshot;
}

/** The snapshot, decoded. */
export interface Snapshot {
  schema: GetSchemaResponse;
  graph: GetGraphResponse;
  target: GetRevisionResponse;
  base?: GetRevisionResponse;
  diff: DiffRevisionsResponse;
  diffFull: DiffRevisionsResponse;
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** The review in a tool result's `structuredContent`, if it is one. */
export function parseReview(structured: unknown): PlanReview | undefined {
  if (!isObject(structured) || !isObject(structured.snapshot)) return undefined;
  const { root, revision, snapshot } = structured;
  if (typeof root !== "string" || typeof revision !== "string") return undefined;
  const fields = ["schema", "graph", "target", "base", "diff", "diff_full"] as const;
  if (!fields.every((f) => typeof snapshot[f] === "string")) return undefined;
  return structured as unknown as PlanReview;
}

export function decodeSnapshot(s: WireSnapshot): Snapshot {
  return {
    schema: fromBinary(GetSchemaResponseSchema, base64Decode(s.schema)),
    graph: fromBinary(GetGraphResponseSchema, base64Decode(s.graph)),
    target: fromBinary(GetRevisionResponseSchema, base64Decode(s.target)),
    base: s.base ? fromBinary(GetRevisionResponseSchema, base64Decode(s.base)) : undefined,
    diff: fromBinary(DiffRevisionsResponseSchema, base64Decode(s.diff)),
    diffFull: fromBinary(DiffRevisionsResponseSchema, base64Decode(s.diff_full)),
  };
}

const notInReview = (what: string) =>
  new ConnectError(`${what} isn't part of the plan review`, Code.NotFound);

/** A factory.v1 client that answers from `review`'s snapshot alone. */
export function snapshotClient(review: PlanReview, snapshot: Snapshot): FactoryClient {
  const base = review.approved_revision ?? "";
  const transport = createRouterTransport(({ service }) => {
    service(FactoryService, {
      getSchema: () => snapshot.schema,
      getGraph: (req) => {
        if (req.root !== review.root) throw notInReview(`the plan ${req.root}`);
        return snapshot.graph;
      },
      getRevision: (req) => {
        if (req.root !== review.root) throw notInReview(`the plan ${req.root}`);
        if (req.rootHash === "" || req.rootHash === review.revision) return snapshot.target;
        if (snapshot.base && req.rootHash === base) return snapshot.base;
        throw notInReview(`revision ${req.rootHash}`);
      },
      diffRevisions: (req) => {
        const target = req.target === "" ? review.revision : req.target;
        if (req.root !== review.root || req.base !== base || target !== review.revision) {
          throw notInReview(`the diff ${req.base}..${req.target}`);
        }
        return req.compact ? snapshot.diff : snapshot.diffFull;
      },
    });
  });
  return createFactoryClient(transport);
}
