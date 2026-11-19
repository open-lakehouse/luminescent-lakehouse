// An in-memory factory.v1 service for tests and Storybook, served through
// `createRouterTransport`, so components run against the same client and
// transport seam they use in production.
//
// It mirrors the server's behavior closely enough to drive the UI: writes,
// a revision per content change, a keyed tree diff, drift and the ready set.
// It is not the server: its hash is FNV-1a, not SHA-256, and it checks only
// the rules the UI exercises (one parent, no cycles, allowed child kinds).

import {
  create,
  createFileRegistry,
  type DescMessage,
  type MessageInitShape,
  toBinary,
} from "@bufbuild/protobuf";
import {
  type Any,
  anyPack,
  FieldDescriptorProto_Label,
  FieldDescriptorProto_Type,
  type FileDescriptorProto,
  FileDescriptorProtoSchema,
  FileDescriptorSetSchema,
  type Timestamp,
  timestampNow,
} from "@bufbuild/protobuf/wkt";
import {
  Code,
  ConnectError,
  createConnectRouter,
  createRouterTransport,
  type Transport,
} from "@connectrpc/connect";
import { createFetchHandler } from "@connectrpc/connect/protocol";
import { createFactoryClient, type FactoryClient } from "./client.js";
import {
  ChangeKind,
  type DiffEntry,
  DiffEntrySchema,
  FactoryService,
  type MerkleNode,
  MerkleNodeSchema,
  type Revision,
  RevisionSchema,
  type TextSection,
  TextSectionSchema,
} from "./gen/factory/v1/service_pb.js";
import {
  ArtifactSource,
  OmnigentSessionSchema,
  PlacementSource,
  StepKind,
  StepState,
} from "./gen/factory/workgraph/v1/execution_pb.js";
import {
  type Condition,
  ConditionSchema,
  type Edge,
  EdgeSchema,
  type Node,
  NodeMetadataSchema,
  NodeSchema,
  type NodeSpec,
  NodeSpecSchema,
  NodeStatusSchema,
  type PayloadKind,
  PayloadKindSchema,
  Phase,
  type Schema,
  SchemaSchema,
  Topology,
} from "./gen/factory/workgraph/v1/graph_pb.js";
import {
  DecisionSchema,
  GateSchema,
  PlanSchema,
  PrMergedSchema,
  TaskSchema,
} from "./gen/factory/workgraph/v1/kinds_pb.js";
import {
  type Annotation,
  AnnotationSchema,
  LearningSchema,
  type Run,
  RunSchema,
  RunState,
} from "./gen/factory/workgraph/v1/records_pb.js";
import {
  builtinFiles,
  CONDITION,
  EDGE,
  fileDescriptorSet,
  KIND,
  packPayload,
  shortKind,
  unpackPayload,
} from "./kinds.js";

/** The server's built-in schema: the built-in extension's kinds and gate
 * conditions, as factory-core's `builtin` module registers them. */
export function builtinSchema(): Schema {
  const items = [KIND.task, KIND.decision, KIND.gate];
  const ext = "builtin";
  const display = (icon: string, tone: string, cardFields: string[] = []) => ({
    icon,
    tone,
    cardFields,
  });
  return create(SchemaSchema, {
    nodeKinds: [
      {
        kind: KIND.plan,
        displayName: "Plan",
        root: true,
        allowedChildKinds: items,
        extension: ext,
        display: display("map", "violet", ["goal"]),
      },
      {
        kind: KIND.task,
        displayName: "Task",
        allowedChildKinds: items,
        extension: ext,
        display: display("list-checks", "sky", ["repo"]),
      },
      {
        kind: KIND.decision,
        displayName: "Decision",
        allowedChildKinds: [],
        extension: ext,
        display: display("split", "amber", ["outcome"]),
      },
      {
        kind: KIND.gate,
        displayName: "Gate",
        allowedChildKinds: [],
        extension: ext,
        display: display("hourglass", "slate"),
      },
    ],
    gateConditions: [
      { typeUrl: CONDITION.prMerged, displayName: "PR merged", extension: ext },
      { typeUrl: CONDITION.human, displayName: "Person", extension: ext },
      { typeUrl: CONDITION.notBefore, displayName: "Not before", extension: ext },
    ],
    edgeKinds: [
      {
        name: EDGE.childOf,
        inverse: "parent_of",
        topology: Topology.TREE,
        fromKinds: items,
        hashed: true,
      },
      {
        name: EDGE.blockedBy,
        inverse: "blocks",
        topology: Topology.DAG,
        fromKinds: items,
        toKinds: items,
        hashed: true,
      },
      { name: EDGE.relatesTo, inverse: "relates_to", topology: Topology.NETWORK },
      {
        name: EDGE.discoveredFrom,
        inverse: "led_to",
        topology: Topology.NETWORK,
        fromKinds: items,
        toKinds: items,
      },
    ],
  });
}

interface StoredRevision {
  revision: Revision;
  tree: MerkleNode;
  text: string;
  sections: TextSection[];
}

interface GraphState {
  root: string;
  nodes: Map<string, Node>;
  edges: Edge[];
  revisions: StoredRevision[];
  approved?: string;
  annotations: Annotation[];
  runs: Run[];
}

function fnv(s: string): string {
  // 64-bit FNV-1a as two 32-bit halves: deterministic, not cryptographic.
  let h1 = 0x811c9dc5;
  let h2 = 0xcbf29ce4;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 0x01000193) >>> 0;
    h2 = Math.imul(h2 ^ c, 0x01000193) >>> 0;
  }
  return h1.toString(16).padStart(8, "0") + h2.toString(16).padStart(8, "0");
}

function bytesKey(spec: NodeSpec | undefined): string {
  if (!spec?.payload) return "";
  return Array.from(spec.payload.value, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** What the fake service holds. Exposed for tests that assert on it. */
export class FakeFactory {
  readonly schema = builtinSchema();
  /** Descriptors of payload types beyond the built-in ones, as an
   * extension's would arrive in GetSchema. */
  readonly extraFiles: FileDescriptorProto[] = [];
  readonly graphs = new Map<string, GraphState>();
  private nextId = 1;
  private sequence = 0n;
  private listeners = new Set<(root: string) => void>();
  actor = "operator";

  /** Register an extension's payload types, as a server built with it
   * would: its descriptors, and its task extensions and gate conditions. */
  extend(ext: {
    files: FileDescriptorProto[];
    taskExtensions?: MessageInitShape<typeof PayloadKindSchema>[];
    gateConditions?: MessageInitShape<typeof PayloadKindSchema>[];
  }) {
    this.extraFiles.push(...ext.files);
    const kind = (k: MessageInitShape<typeof PayloadKindSchema>): PayloadKind =>
      create(PayloadKindSchema, k);
    this.schema.taskExtensions.push(...(ext.taskExtensions ?? []).map(kind));
    this.schema.gateConditions.push(...(ext.gateConditions ?? []).map(kind));
  }

  /** The encoded descriptor set GetSchema returns. */
  descriptors(): Uint8Array {
    const set = fileDescriptorSet(builtinFiles);
    set.file.push(...this.extraFiles);
    return toBinary(FileDescriptorSetSchema, set);
  }

  private id(): string {
    return `mem:${String(this.nextId++).padStart(6, "0")}`;
  }

  private now(): Timestamp {
    return timestampNow();
  }

  graphOf(ref: string): GraphState {
    for (const g of this.graphs.values()) if (g.nodes.has(ref)) return g;
    throw new ConnectError(`no node ${ref}`, Code.NotFound);
  }

  private childrenOf(g: GraphState, ref: string): string[] {
    return g.edges
      .filter((e) => e.kind === EDGE.childOf && e.to === ref)
      .map((e) => e.from)
      .sort();
  }

  private descendants(g: GraphState, ref: string): string[] {
    const out: string[] = [];
    const stack = this.childrenOf(g, ref);
    while (stack.length) {
      const n = stack.pop() as string;
      out.push(n);
      stack.push(...this.childrenOf(g, n));
    }
    return out;
  }

  // --- canonical forms ---------------------------------------------------

  tree(g: GraphState): MerkleNode {
    const build = (ref: string): MerkleNode => {
      const n = g.nodes.get(ref) as Node;
      const blockers = g.edges
        .filter((e) => e.kind === EDGE.blockedBy && e.from === ref)
        .map((e) => e.to)
        .sort();
      const nodeHash = fnv(
        [n.kind, n.spec?.title.trim(), n.spec?.body.trim(), bytesKey(n.spec), ...blockers].join(
          "\0",
        ),
      );
      const children = this.childrenOf(g, ref).map(build);
      const subtreeHash = children.length
        ? fnv([nodeHash, ...children.map((c) => c.subtreeHash)].join("\0"))
        : nodeHash;
      return create(MerkleNodeSchema, {
        key: ref,
        kind: shortKind(n.kind),
        nodeHash,
        subtreeHash,
        label: n.spec?.title ?? "",
        children,
        attrs: { title: n.spec?.title ?? "", kind: shortKind(n.kind) },
      });
    };
    return build(g.root);
  }

  private render(g: GraphState): { text: string; sections: TextSection[] } {
    const lines: string[] = [];
    const sections: TextSection[] = [];
    const walk = (ref: string, depth: number) => {
      const n = g.nodes.get(ref) as Node;
      if (lines.length) lines.push("");
      const start = lines.length + 1;
      lines.push(`${"#".repeat(Math.min(depth, 6))} ${shortKind(n.kind)}: ${n.spec?.title ?? ""}`);
      const blockers = g.edges.filter((e) => e.kind === EDGE.blockedBy && e.from === ref);
      if (blockers.length) {
        lines.push(`blocked by: ${blockers.map((e) => g.nodes.get(e.to)?.spec?.title).join("; ")}`);
      }
      const payload = unpackPayload(n.spec?.payload) as Record<string, unknown> | undefined;
      for (const [k, v] of Object.entries(payload ?? {})) {
        if (k === "$typeName" || k === "$unknown") continue;
        if (Array.isArray(v)) {
          if (v.length === 0) continue;
          lines.push(`${k}:`, ...v.map((x) => `- ${String(x)}`));
        } else if (v !== "" && v !== undefined) lines.push(`${k}: ${String(v)}`);
      }
      if (n.spec?.body) lines.push("", ...n.spec.body.trim().split("\n"));
      sections.push(
        create(TextSectionSchema, { key: ref, startLine: start, endLine: lines.length }),
      );
      for (const c of this.childrenOf(g, ref)) walk(c, depth + 1);
    };
    walk(g.root, 1);
    return { text: `${lines.join("\n")}\n`, sections };
  }

  private record(g: GraphState): string {
    const tree = this.tree(g);
    const last = g.revisions.at(-1);
    if (last?.tree.subtreeHash === tree.subtreeHash) return tree.subtreeHash;
    const changes: Record<string, number> = {};
    for (const e of compact(diff(last?.tree, tree), last?.tree, tree)) {
      const name = changeName(e.change);
      changes[name] = (changes[name] ?? 0) + 1;
    }
    const { text, sections } = this.render(g);
    g.revisions.push({
      revision: create(RevisionSchema, {
        rootHash: tree.subtreeHash,
        parentRootHash: last?.revision.rootHash ?? "",
        sequence: BigInt(g.revisions.length + 1),
        provenance: { actor: this.actor, via: "control-room" },
        createdAt: this.now(),
        changes,
      }),
      tree,
      text,
      sections,
    });
    return tree.subtreeHash;
  }

  private changed(g: GraphState) {
    this.record(g);
    this.sequence += 1n;
    for (const l of this.listeners) l(g.root);
  }

  subscribe(listener: (root: string) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  currentSequence(): bigint {
    return this.sequence;
  }

  revision(g: GraphState, hash?: string): StoredRevision {
    const rev = hash
      ? // Not findLast: the desktop, whose Playwright build serves this fake, targets ES2022.
        [...g.revisions].reverse().find((r) => r.revision.rootHash === hash)
      : g.revisions.at(-1);
    if (!rev) throw new ConnectError(`no revision ${hash}`, Code.NotFound);
    return rev;
  }

  drifted(g: GraphState): string[] {
    if (!g.approved) return [];
    const current = this.tree(g);
    if (current.subtreeHash === g.approved) return [];
    const base = this.revision(g, g.approved).tree;
    return compact(diff(base, current), base, current).map((e) => e.key);
  }

  /** The graph's nodes with derived conditions. */
  withConditions(g: GraphState): Node[] {
    const drifted = new Set(this.drifted(g));
    const phase = (r: string) => g.nodes.get(r)?.status?.phase ?? Phase.UNSPECIFIED;
    const resolved = (r: string) => phase(r) === Phase.DONE || phase(r) === Phase.DROPPED;
    const now = Date.now() / 1000;
    return [...g.nodes.values()].map((n) => {
      const conds: Condition[] = [];
      const open = g.edges
        .filter((e) => e.kind === EDGE.blockedBy && e.from === n.ref && !resolved(e.to))
        .map((e) => e.to);
      if (open.length) {
        conds.push(
          create(ConditionSchema, {
            type: "Blocked",
            status: true,
            reason: "OpenBlockers",
            message: `waiting on ${open.map((r) => g.nodes.get(r)?.spec?.title).join(", ")}`,
          }),
        );
      }
      const run = g.runs.find(
        (r) =>
          r.node === n.ref &&
          [RunState.QUEUED, RunState.WORKING, RunState.INPUT_REQUIRED].includes(r.state),
      );
      const live = run ? Number(run.leaseExpiry?.seconds ?? 0n) > now : undefined;
      if (live === true) {
        conds.push(create(ConditionSchema, { type: "Running", status: true, reason: "LiveRun" }));
        if (run?.stage === "in_review" || run?.stage === "fixing")
          conds.push(
            create(ConditionSchema, { type: "InReview", status: true, reason: "RunInReview" }),
          );
        if (run?.stage === "awaiting_merge" || run?.stage === "awaiting_gate")
          conds.push(
            create(ConditionSchema, { type: "AwaitingGate", status: true, reason: "RunAwaiting" }),
          );
      }
      if (live === false)
        conds.push(
          create(ConditionSchema, { type: "Stalled", status: true, reason: "LeaseExpired" }),
        );
      if (drifted.has(n.ref)) {
        conds.push(
          create(ConditionSchema, {
            type: "Drifted",
            status: true,
            reason: "ChangedSinceApproval",
          }),
        );
      }
      if (n.ref !== g.root && !resolved(n.ref)) {
        const openChildren = this.childrenOf(g, n.ref).some((c) => !resolved(c));
        let ready = false;
        let reason = "Dispatchable";
        if (phase(n.ref) !== Phase.APPROVED) reason = "NotApproved";
        else if (open.length) reason = "OpenBlockers";
        else if (run) reason = "HasRun";
        else if (openChildren) reason = "OpenChildren";
        else if (drifted.has(n.ref)) reason = "Drifted";
        else ready = true;
        conds.unshift(create(ConditionSchema, { type: "Ready", status: ready, reason }));
      }
      return create(NodeSchema, {
        ...n,
        status: create(NodeStatusSchema, { phase: phase(n.ref), conditions: conds }),
      });
    });
  }

  // --- writes ------------------------------------------------------------

  createNode(parent: string, kind: string, spec: NodeSpec): Node {
    const nodeKind = this.schema.nodeKinds.find((k) => k.kind === kind);
    if (!nodeKind) throw new ConnectError(`unknown kind ${kind}`, Code.InvalidArgument);
    if (!spec.title.trim()) throw new ConnectError("a node needs a title", Code.InvalidArgument);
    const ref = this.id();
    const node = create(NodeSchema, {
      ref,
      kind,
      spec,
      status: { phase: Phase.PROPOSED },
      metadata: { backend: "memory", etag: "1", createdBy: this.actor, createdAt: this.now() },
    });
    if (!parent) {
      if (!nodeKind.root)
        throw new ConnectError(`${shortKind(kind)} can't be a root`, Code.InvalidArgument);
      const g: GraphState = {
        root: ref,
        nodes: new Map([[ref, node]]),
        edges: [],
        revisions: [],
        annotations: [],
        runs: [],
      };
      this.graphs.set(ref, g);
      this.changed(g);
      return node;
    }
    const g = this.graphOf(parent);
    const pkind = this.schema.nodeKinds.find((k) => k.kind === g.nodes.get(parent)?.kind);
    if (!pkind?.allowedChildKinds.includes(kind)) {
      throw new ConnectError(
        `a ${shortKind(pkind?.kind ?? "")} can't have a ${shortKind(kind)} child`,
        Code.InvalidArgument,
      );
    }
    g.nodes.set(ref, node);
    g.edges.push(create(EdgeSchema, { kind: EDGE.childOf, from: ref, to: parent }));
    this.changed(g);
    return node;
  }

  updateNode(ref: string, spec: NodeSpec | undefined, etag: string): Node {
    const g = this.graphOf(ref);
    const node = g.nodes.get(ref) as Node;
    if (etag && node.metadata?.etag !== etag) {
      throw new ConnectError(`${ref} changed since etag ${etag}`, Code.Aborted);
    }
    const next = create(NodeSchema, {
      ...node,
      spec: spec ?? node.spec,
      metadata: create(NodeMetadataSchema, {
        labels: node.metadata?.labels,
        backend: node.metadata?.backend,
        createdBy: node.metadata?.createdBy,
        createdAt: node.metadata?.createdAt,
        etag: String(Number(node.metadata?.etag ?? "0") + 1),
        updatedAt: this.now(),
      }),
    });
    g.nodes.set(ref, next);
    this.changed(g);
    return next;
  }

  moveNode(ref: string, newParent: string): Node {
    const g = this.graphOf(ref);
    if (ref === g.root) throw new ConnectError("the root can't be moved", Code.FailedPrecondition);
    if (ref === newParent || this.descendants(g, ref).includes(newParent)) {
      throw new ConnectError("a node can't move below itself", Code.FailedPrecondition);
    }
    if (!g.nodes.has(newParent))
      throw new ConnectError("a node can only move within its graph", Code.FailedPrecondition);
    const pkind = this.schema.nodeKinds.find((k) => k.kind === g.nodes.get(newParent)?.kind);
    if (!pkind?.allowedChildKinds.includes(g.nodes.get(ref)?.kind ?? "")) {
      throw new ConnectError("that parent can't hold this kind", Code.InvalidArgument);
    }
    const e = g.edges.find((x) => x.kind === EDGE.childOf && x.from === ref);
    if (e) e.to = newParent;
    this.changed(g);
    return g.nodes.get(ref) as Node;
  }

  deleteNode(ref: string, recursive: boolean) {
    const g = this.graphOf(ref);
    const below = this.descendants(g, ref);
    if (below.length && !recursive) {
      throw new ConnectError(`${ref} has nodes below it`, Code.FailedPrecondition);
    }
    const gone = new Set([ref, ...below]);
    if (ref === g.root) {
      this.graphs.delete(ref);
      return;
    }
    for (const r of gone) g.nodes.delete(r);
    g.edges = g.edges.filter((e) => !gone.has(e.from) && !gone.has(e.to));
    this.changed(g);
  }

  addEdge(edge: Edge) {
    const kind = this.schema.edgeKinds.find((k) => k.name === edge.kind);
    if (!kind) throw new ConnectError(`unknown edge kind ${edge.kind}`, Code.InvalidArgument);
    if (kind.topology === Topology.TREE) {
      throw new ConnectError("the hierarchy changes through create and move", Code.InvalidArgument);
    }
    if (edge.from === edge.to) throw new ConnectError("no self-edges", Code.InvalidArgument);
    const g = this.graphOf(edge.from);
    if (!g.nodes.has(edge.to))
      throw new ConnectError("an edge can't join two graphs", Code.InvalidArgument);
    const allows = (list: string[], k: string) => list.length === 0 || list.includes(k);
    if (
      !allows(kind.fromKinds, g.nodes.get(edge.from)?.kind ?? "") ||
      !allows(kind.toKinds, g.nodes.get(edge.to)?.kind ?? "")
    ) {
      throw new ConnectError(`${edge.kind} can't join those kinds`, Code.InvalidArgument);
    }
    if (g.edges.some((e) => e.kind === edge.kind && e.from === edge.from && e.to === edge.to)) {
      throw new ConnectError("the edge exists", Code.AlreadyExists);
    }
    if (kind.topology === Topology.DAG) {
      const stack = [edge.to];
      const seen = new Set<string>();
      while (stack.length) {
        const n = stack.pop() as string;
        if (n === edge.from)
          throw new ConnectError(`${edge.kind} would form a cycle`, Code.FailedPrecondition);
        if (seen.has(n)) continue;
        seen.add(n);
        for (const e of g.edges) if (e.kind === edge.kind && e.from === n) stack.push(e.to);
      }
    }
    g.edges.push(create(EdgeSchema, { kind: edge.kind, from: edge.from, to: edge.to }));
    this.changed(g);
  }

  removeEdge(edge: Edge) {
    const g = this.graphOf(edge.from);
    const before = g.edges.length;
    g.edges = g.edges.filter(
      (e) => !(e.kind === edge.kind && e.from === edge.from && e.to === edge.to),
    );
    if (g.edges.length === before) throw new ConnectError("no such edge", Code.NotFound);
    this.changed(g);
  }

  setPhase(ref: string, phase: Phase): Node {
    const g = this.graphOf(ref);
    const node = g.nodes.get(ref) as Node;
    const next = create(NodeSchema, { ...node, status: create(NodeStatusSchema, { phase }) });
    g.nodes.set(ref, next);
    this.changed(g);
    return next;
  }

  approve(root: string, hash: string): StoredRevision {
    const g = this.graphs.get(root);
    if (!g) throw new ConnectError(`no graph ${root}`, Code.NotFound);
    const current = this.record(g);
    if (current !== hash)
      throw new ConnectError(`${hash} isn't the current revision`, Code.FailedPrecondition);
    for (const [ref, n] of g.nodes) {
      if (n.status?.phase === Phase.PROPOSED) {
        g.nodes.set(
          ref,
          create(NodeSchema, { ...n, status: create(NodeStatusSchema, { phase: Phase.APPROVED }) }),
        );
      }
    }
    g.approved = hash;
    this.changed(g);
    return this.revision(g, hash);
  }

  annotate(node: string, kind: string, payload: Annotation["payload"]): Annotation {
    const g = this.graphOf(node);
    const a = create(AnnotationSchema, {
      ref: this.id(),
      node,
      kind,
      payload,
      provenance: { actor: this.actor, via: "control-room" },
      createdAt: this.now(),
    });
    g.annotations.push(a);
    this.sequence += 1n;
    for (const l of this.listeners) l(g.root);
    return a;
  }

  putRun(run: Run) {
    const g = this.graphOf(run.node);
    g.runs = [...g.runs.filter((r) => r.ref !== run.ref), run];
  }
}

function changeName(c: ChangeKind): string {
  return (
    {
      [ChangeKind.ADDED]: "added",
      [ChangeKind.REMOVED]: "removed",
      [ChangeKind.MODIFIED]: "modified",
      [ChangeKind.MODIFIED_DESCENDANTS]: "modified-descendants",
      [ChangeKind.MOVED]: "moved",
      [ChangeKind.UNSPECIFIED]: "unspecified",
    } as Record<ChangeKind, string>
  )[c];
}

interface Indexed {
  node: MerkleNode;
  parent: string;
}

function index(root: MerkleNode): Map<string, Indexed> {
  const m = new Map<string, Indexed>();
  const walk = (n: MerkleNode) => {
    for (const c of n.children) {
      m.set(c.key, { node: c, parent: n.key });
      walk(c);
    }
  };
  walk(root);
  return m;
}

/** The keyed tree diff, with the server's key-sorted semantics. */
export function diff(before: MerkleNode | undefined, after: MerkleNode | undefined): DiffEntry[] {
  if (!after) return [];
  const b = index(after);
  type Extra = { contentChanged?: boolean; fromParent?: string; toParent?: string };
  const entry = (n: MerkleNode, change: ChangeKind, extra: Extra = {}) =>
    create(DiffEntrySchema, { key: n.key, kind: n.kind, label: n.label, change, ...extra });
  if (!before) return [...b.values()].map(({ node }) => entry(node, ChangeKind.ADDED));
  if (before.subtreeHash === after.subtreeHash) return [];
  const a = index(before);
  const out: DiffEntry[] = [];
  for (const [key, cur] of b) {
    const prev = a.get(key);
    if (!prev) {
      out.push(entry(cur.node, ChangeKind.ADDED));
      continue;
    }
    const moved = prev.parent !== cur.parent;
    const own = prev.node.nodeHash !== cur.node.nodeHash;
    if (prev.node.subtreeHash === cur.node.subtreeHash && !moved) continue;
    const change = moved
      ? ChangeKind.MOVED
      : own
        ? ChangeKind.MODIFIED
        : ChangeKind.MODIFIED_DESCENDANTS;
    out.push(
      entry(cur.node, change, {
        contentChanged: own,
        ...(moved ? { fromParent: prev.parent, toParent: cur.parent } : {}),
      }),
    );
  }
  for (const [key, prev] of a) if (!b.has(key)) out.push(entry(prev.node, ChangeKind.REMOVED));
  return out;
}

/** Drop descendant-only entries; collapse added/removed subtrees. */
export function compact(
  entries: DiffEntry[],
  before?: MerkleNode,
  after?: MerkleNode,
): DiffEntry[] {
  const bi = before ? index(before) : new Map<string, Indexed>();
  const ai = after ? index(after) : new Map<string, Indexed>();
  const changed = new Map(entries.map((e) => [e.key, e.change]));
  return entries.filter((e) => {
    if (e.change === ChangeKind.MODIFIED_DESCENDANTS) return false;
    if (e.change !== ChangeKind.ADDED && e.change !== ChangeKind.REMOVED) return true;
    const idx = e.change === ChangeKind.REMOVED ? bi : ai;
    let p = idx.get(e.key)?.parent;
    while (p) {
      if (changed.get(p) === e.change) return false;
      p = idx.get(p)?.parent;
    }
    return true;
  });
}

function routes(fake: FakeFactory) {
  const graph = (root: string) => {
    const g = fake.graphs.get(root);
    if (!g) throw new ConnectError(`no graph ${root}`, Code.NotFound);
    return g;
  };
  const rev = (g: GraphState, r: StoredRevision) =>
    create(RevisionSchema, { ...r.revision, approved: r.revision.rootHash === g.approved });
  return {
    getSchema: () => ({ schema: fake.schema, fileDescriptorSet: fake.descriptors() }),
    listGraphs: () => ({
      graphs: [...fake.graphs.values()].map((g) => ({
        root: fake.withConditions(g).find((n) => n.ref === g.root),
        nodeCount: g.nodes.size,
        revision: fake.tree(g).subtreeHash,
        approvedRevision: g.approved ?? "",
        driftedCount: fake.drifted(g).length,
      })),
    }),
    getGraph: (req: { root: string }) => {
      const g = graph(req.root);
      return {
        graph: { root: g.root, nodes: fake.withConditions(g), edges: g.edges },
        revision: fake.tree(g).subtreeHash,
        approvedRevision: g.approved ?? "",
        drifted: fake.drifted(g),
      };
    },
    async *watchGraph(req: { root: string }, ctx: { signal: AbortSignal }) {
      const g = graph(req.root);
      yield { revision: fake.tree(g).subtreeHash, sequence: fake.currentSequence() };
      const queue: string[] = [];
      let wake: (() => void) | undefined;
      const off = fake.subscribe((root) => {
        if (root !== req.root) return;
        queue.push(root);
        wake?.();
      });
      try {
        while (!ctx.signal.aborted) {
          if (!queue.length) {
            await new Promise<void>((resolve) => {
              wake = resolve;
              ctx.signal.addEventListener("abort", () => resolve(), { once: true });
            });
            continue;
          }
          queue.shift();
          const cur = fake.graphs.get(req.root);
          if (!cur) return;
          yield { revision: fake.tree(cur).subtreeHash, sequence: fake.currentSequence() };
        }
      } finally {
        off();
      }
    },
    createNode: (req: { parent: string; kind: string; spec?: NodeSpec }) => ({
      node: fake.createNode(req.parent, req.kind, req.spec ?? create(NodeSpecSchema)),
    }),
    updateNode: (req: { ref: string; spec?: NodeSpec; etag: string }) => ({
      node: fake.updateNode(req.ref, req.spec, req.etag),
    }),
    moveNode: (req: { ref: string; newParent: string }) => ({
      node: fake.moveNode(req.ref, req.newParent),
    }),
    deleteNode: (req: { ref: string; recursive: boolean }) => {
      fake.deleteNode(req.ref, req.recursive);
      return {};
    },
    addEdge: (req: { edge?: Edge }) => {
      if (!req.edge) throw new ConnectError("edge is required", Code.InvalidArgument);
      fake.addEdge(req.edge);
      return {};
    },
    removeEdge: (req: { edge?: Edge }) => {
      if (!req.edge) throw new ConnectError("edge is required", Code.InvalidArgument);
      fake.removeEdge(req.edge);
      return {};
    },
    setPhase: (req: { ref: string; phase: Phase }) => ({ node: fake.setPhase(req.ref, req.phase) }),
    annotate: (req: { node: string; kind: string; payload?: Annotation["payload"] }) => ({
      annotation: fake.annotate(req.node, req.kind, req.payload),
    }),
    listAnnotations: (req: { node: string }) => ({
      annotations: fake.graphOf(req.node).annotations.filter((a) => a.node === req.node),
    }),
    listRuns: (req: { root: string }) => ({ runs: graph(req.root).runs }),
    getRun: (req: { root: string; run: string }) => {
      const run = graph(req.root).runs.find((r) => r.ref === req.run);
      if (!run) throw new ConnectError(`no run ${req.run}`, Code.NotFound);
      return { run };
    },
    listRevisions: (req: { root: string }) => {
      const g = graph(req.root);
      return { revisions: [...g.revisions].reverse().map((r) => rev(g, r)) };
    },
    getRevision: (req: { root: string; rootHash: string }) => {
      const g = graph(req.root);
      const r = fake.revision(g, req.rootHash || undefined);
      return { revision: rev(g, r), tree: r.tree, text: r.text, sections: r.sections };
    },
    diffRevisions: (req: { root: string; base: string; target: string; compact: boolean }) => {
      const g = graph(req.root);
      const target = fake.revision(g, req.target || undefined).tree;
      const base = req.base ? fake.revision(g, req.base).tree : undefined;
      const raw = diff(base, target);
      return { entries: req.compact ? compact(raw, base, target) : raw };
    },
    approveRevision: (req: { root: string; rootHash: string }) => {
      const g = graph(req.root);
      return { revision: rev(g, fake.approve(req.root, req.rootHash)) };
    },
  };
}

export interface FakeFactoryHandle {
  fake: FakeFactory;
  transport: Transport;
  client: FactoryClient;
}

/** An in-memory factory service. With `seed: "demo"`, it holds the demo plan. */
export function createFakeFactory(options: { seed?: "demo" } = {}): FakeFactoryHandle {
  const fake = new FakeFactory();
  if (options.seed === "demo") seedDemo(fake);
  const transport = createRouterTransport(({ service }) => {
    // The handlers are typed loosely above; the router checks shapes at runtime.
    service(FactoryService, routes(fake) as never);
  });
  return { fake, transport, client: createFactoryClient(transport) };
}

/**
 * An in-memory factory service over `fetch`: Connect's HTTP protocol (binary or JSON)
 * at `/factory.v1.FactoryService/<Method>`, as factory-server serves it. For a host
 * whose binding is a `fetch` (the desktop's mocked IPC in its Playwright build).
 */
export function createFakeFactoryFetch(options: { seed?: "demo" } = {}): {
  fake: FakeFactory;
  fetch: (request: Request) => Promise<Response>;
} {
  const fake = new FakeFactory();
  if (options.seed === "demo") seedDemo(fake);
  const router = createConnectRouter().service(FactoryService, routes(fake) as never);
  const handlers = new Map(
    router.handlers.map((h) => [h.requestPath, createFetchHandler(h)] as const),
  );
  return {
    fake,
    fetch: async (request) => {
      const handler = handlers.get(new URL(request.url).pathname);
      return handler ? handler(request) : new Response(null, { status: 404 });
    },
  };
}

const spec = (title: string, payload: NodeSpec["payload"], body = "") =>
  create(NodeSpecSchema, { title, body, payload });

const REPO = "open-lakehouse/luminescent-lakehouse";

const task = (title: string, acceptance: string, touchedPaths: string[]) =>
  spec(
    title,
    packPayload(TaskSchema, {
      acceptance,
      touchedPaths,
      repo: REPO,
      profile: "executor",
      reviewProfile: "reviewer",
    }),
  );

const secs = (offset: number) => ({ seconds: BigInt(Math.floor(Date.now() / 1000) + offset) });

/** A session step, as the reconciler records one. */
const sessionStep = (
  index: number,
  name: string,
  state: StepState,
  session: {
    id: string;
    profile: string;
    agent: string;
    harness: string;
    model: string;
    status: string;
    cost: number;
  },
) => ({
  index,
  name,
  kind: StepKind.SESSION,
  state,
  payload: packPayload(OmnigentSessionSchema, {
    sessionId: session.id,
    status: session.status,
    costUsd: session.cost,
    spec: {
      profile: session.profile,
      agentId: `ag_${session.agent}`,
      agentName: session.agent,
      agentVersion: "4",
      harness: session.harness,
      model: session.model,
      hostType: "external",
      hostId: "local",
      branch: "factory/olai-core",
      baseBranch: "main",
      environment: "local",
      placementSource: PlacementSource.REGISTRY,
    },
  }),
});

const groupedPr = (number: number, layer: number, title: string, base: string) => ({
  step: 0,
  source: ArtifactSource.REPORTED,
  artifact: {
    case: "pullRequest" as const,
    value: {
      url: `https://github.com/${REPO}/pull/${number}`,
      repo: REPO,
      number,
      title,
      head: `factory/olai-core-${layer}`,
      base,
      state: "open",
      checks: "passing",
      reviewDecision: "review_required",
      group: { name: "olai-core", position: layer, of: 2 },
    },
  },
});

/** The demo plan, as factory-core's `demo::seed` builds it. Returns its root. */
export function seedDemo(fake: FakeFactory): string {
  fake.actor = "factory-planner";
  const root = fake.createNode(
    "",
    KIND.plan,
    spec(
      "Typed work graph on olai-store",
      packPayload(PlanSchema, {
        goal: "An approved plan runs end to end on olai-store, with no GitHub issues.",
        rubric: [
          "The conformance suite passes on every backend.",
          "Every revision names its actor.",
        ],
        criteria: ["AGENTS.md"],
        repos: [{ slug: REPO, defaultBranch: "main", criteria: "AGENTS.md" }],
        plannerProfile: "planner",
      }),
      "Store the factory's plans in olai-store and show them in the control room.",
    ),
  ).ref;
  const protos = fake.createNode(
    root,
    KIND.task,
    task("Define factory.workgraph.v1", "The node envelope, edge kinds and schema are protos.", [
      "crates/factory-proto/proto/**",
    ]),
  ).ref;
  const core = fake.createNode(
    root,
    KIND.task,
    task(
      "Build the core on olai-store",
      "Writes are validated and every change records a revision.",
      ["crates/factory-core/**"],
    ),
  ).ref;
  const server = fake.createNode(
    root,
    KIND.task,
    task(
      "Serve factory.v1 over Connect",
      "The control room reads and edits plans only through FactoryService.",
      ["crates/factory-server/**"],
    ),
  ).ref;
  const ui = fake.createNode(
    root,
    KIND.task,
    task("Control-room UI", "Plans render as graphs, and any two revisions diff.", [
      "packages/factory-ui/**",
    ]),
  ).ref;
  fake.addEdge(create(EdgeSchema, { kind: EDGE.blockedBy, from: core, to: protos }));
  fake.addEdge(create(EdgeSchema, { kind: EDGE.blockedBy, from: server, to: core }));
  const decision = fake.createNode(
    root,
    KIND.decision,
    spec(
      "Where do revisions live?",
      packPayload(DecisionSchema, {
        question: "Store revisions in the work-graph backend or a separate factory store?",
        options: ["same olai-store", "separate SQLite"],
      }),
    ),
  ).ref;
  fake.addEdge(create(EdgeSchema, { kind: EDGE.blockedBy, from: core, to: decision }));
  const gate = fake.createNode(
    root,
    KIND.gate,
    spec(
      "The protos are merged",
      packPayload(GateSchema, {
        condition: packPayload(PrMergedSchema, { task: protos }),
      }),
      "The core builds on the generated types.",
    ),
  ).ref;
  fake.addEdge(create(EdgeSchema, { kind: EDGE.blockedBy, from: core, to: gate }));

  fake.actor = "operator";
  const g = fake.graphOf(root);
  fake.approve(root, fake.tree(g).subtreeHash);
  fake.setPhase(protos, Phase.DONE);
  fake.setPhase(gate, Phase.DONE);
  fake.updateNode(
    decision,
    spec(
      "Where do revisions live?",
      packPayload(DecisionSchema, {
        question: "Store revisions in the work-graph backend or a separate factory store?",
        options: ["same olai-store", "separate SQLite"],
        outcome: "same olai-store",
      }),
    ),
    "",
  );
  fake.setPhase(decision, Phase.DONE);

  fake.actor = "factory-executor";
  fake.putRun(
    create(RunSchema, {
      ref: "mem:run-1",
      node: core,
      attempt: 1,
      state: RunState.WORKING,
      sessionId: "sess-demo-2",
      leaseExpiry: secs(3600),
      promptName: "factory.dispatch.executor",
      promptVersion: "3",
      createdAt: secs(-1800),
      updatedAt: secs(-60),
      executor: "task.v1",
      stage: "in_review",
      steps: [
        sessionStep(0, "implement", StepState.SUCCEEDED, {
          id: "sess-demo-1",
          profile: "executor",
          agent: "factory-executor",
          harness: "claude-sdk",
          model: "opus",
          status: "idle",
          cost: 3.2,
        }),
        sessionStep(1, "review", StepState.ACTIVE, {
          id: "sess-demo-2",
          profile: "reviewer",
          agent: "factory-reviewer",
          harness: "codex",
          model: "gpt-5.5",
          status: "running",
          cost: 0.4,
        }),
      ],
      artifacts: [
        groupedPr(101, 1, "feat(factory): store work graphs in olai-store", "main"),
        groupedPr(102, 2, "feat(factory): record a revision on every write", "factory/olai-core-1"),
      ],
      costUsd: 3.6,
    }),
  );
  const adapter = fake.createNode(
    core,
    KIND.task,
    task("olai-store adapter", "Nodes are objects named [graph, node].", [
      "crates/factory-core/src/olai.rs",
    ]),
  ).ref;
  fake.createNode(
    core,
    KIND.task,
    task("Revision log", "Revisions chain by parent root hash.", [
      "crates/factory-core/src/factory.rs",
    ]),
  );
  fake.annotate(
    core,
    KIND.learning,
    packPayload(LearningSchema, {
      lesson: "olai-store deletes an object's associations with it.",
      evidence: ["trestle/crates/olai-store/src/store.rs"],
    }),
  );

  fake.actor = "factory-planner";
  const schemaTask = fake.createNode(
    root,
    KIND.task,
    task("Serve the schema", "GetSchema returns every node and edge kind.", [
      "crates/factory-server/src/lib.rs",
    ]),
  ).ref;
  fake.moveNode(schemaTask, server);
  fake.updateNode(
    server,
    task(
      "Serve factory.v1 over Connect",
      "The control room reads and edits plans only through FactoryService, over Connect JSON and binary.",
      ["crates/factory-server/**"],
    ),
    "",
  );
  fake.addEdge(create(EdgeSchema, { kind: EDGE.blockedBy, from: ui, to: server }));
  fake.addEdge(create(EdgeSchema, { kind: EDGE.discoveredFrom, from: adapter, to: core }));
  fake.addEdge(create(EdgeSchema, { kind: EDGE.relatesTo, from: ui, to: decision }));
  fake.actor = "operator";
  return root;
}

/** A stand-in for an extension this client has no code for: an
 * `example.v1.Note` message (`text`, `count`) known only by its descriptor,
 * as a private extension's payloads reach the control room. */
export function exampleExtension() {
  const file = create(FileDescriptorProtoSchema, {
    name: "example/v1/note.proto",
    package: "example.v1",
    syntax: "proto3",
    messageType: [
      {
        name: "Note",
        field: [
          {
            name: "text",
            jsonName: "text",
            number: 1,
            type: FieldDescriptorProto_Type.STRING,
            label: FieldDescriptorProto_Label.OPTIONAL,
          },
          {
            name: "count",
            jsonName: "count",
            number: 2,
            type: FieldDescriptorProto_Type.UINT32,
            label: FieldDescriptorProto_Label.OPTIONAL,
          },
        ],
      },
    ],
  });
  const note = createFileRegistry(file, () => undefined).getMessage("example.v1.Note");
  if (!note) throw new Error("example.v1.Note doesn't resolve");
  const typeUrl = "type.googleapis.com/example.v1.Note";
  return {
    file,
    typeUrl,
    /** A Note packed into an `Any`. */
    pack: (text: string, count: number): Any =>
      anyPack(note as DescMessage, create(note as DescMessage, { text, count })),
    /** Register it on `fake` as both a task extension and a gate condition. */
    register(fake: FakeFactory) {
      fake.extend({
        files: [file],
        taskExtensions: [{ typeUrl, displayName: "Note", extension: "example" }],
        gateConditions: [{ typeUrl, displayName: "Noted", extension: "example" }],
      });
    },
  };
}
