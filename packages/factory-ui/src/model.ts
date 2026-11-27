// Helpers over the normalized graph and the Merkle diff, shared by the views.

import { anyIs, anyUnpack, type Timestamp, timestampDate } from "@bufbuild/protobuf/wkt";
import {
  ChangeKind,
  type DiffEntry,
  EDGE,
  type Edge,
  type EdgeKind,
  type EnvironmentPlaces,
  type Graph,
  type MerkleNode,
  type Node,
  type OmnigentSession,
  OmnigentSessionSchema,
  Phase,
  PlacementSource,
  type PullRequest,
  type Run,
  type Schema,
  shortKind,
  Topology,
} from "@open-lakehouse/factory-client";

/** The graph indexed by ref, with its `child_of` hierarchy. */
export interface GraphIndex {
  root: string;
  nodes: Map<string, Node>;
  parent: Map<string, string>;
  children: Map<string, string[]>;
  edges: Edge[];
}

export function indexGraph(graph: Graph | undefined): GraphIndex {
  const nodes = new Map((graph?.nodes ?? []).map((n) => [n.ref, n]));
  const parent = new Map<string, string>();
  const children = new Map<string, string[]>();
  for (const e of graph?.edges ?? []) {
    if (e.kind !== EDGE.childOf) continue;
    parent.set(e.from, e.to);
    children.set(e.to, [...(children.get(e.to) ?? []), e.from].sort());
  }
  return { root: graph?.root ?? "", nodes, parent, children, edges: graph?.edges ?? [] };
}

/** Whether `ref` is `ancestor` or below it. */
export function isWithin(idx: GraphIndex, ref: string, ancestor: string): boolean {
  let cur: string | undefined = ref;
  while (cur) {
    if (cur === ancestor) return true;
    cur = idx.parent.get(cur);
  }
  return false;
}

export function condition(node: Node | undefined, type: string) {
  return node?.status?.conditions.find((c) => c.type === type);
}

export const PHASE_LABEL: Record<Phase, string> = {
  [Phase.UNSPECIFIED]: "—",
  [Phase.PROPOSED]: "proposed",
  [Phase.APPROVED]: "approved",
  [Phase.DONE]: "done",
  [Phase.DROPPED]: "dropped",
};

export const CHANGE_LABEL: Record<ChangeKind, string> = {
  [ChangeKind.UNSPECIFIED]: "—",
  [ChangeKind.ADDED]: "added",
  [ChangeKind.REMOVED]: "removed",
  [ChangeKind.MODIFIED]: "modified",
  [ChangeKind.MODIFIED_DESCENDANTS]: "changed below",
  [ChangeKind.MOVED]: "moved",
};

/** Tailwind classes per change: a tinted background and a left rule. */
export const CHANGE_TONE: Record<ChangeKind, string> = {
  [ChangeKind.UNSPECIFIED]: "",
  [ChangeKind.ADDED]:
    "bg-emerald-500/10 border-l-emerald-500 text-emerald-900 dark:text-emerald-200",
  [ChangeKind.REMOVED]:
    "bg-rose-500/10 border-l-rose-500 text-rose-900 line-through decoration-rose-500/60 dark:text-rose-200",
  [ChangeKind.MODIFIED]: "bg-amber-500/10 border-l-amber-500 text-amber-900 dark:text-amber-200",
  [ChangeKind.MODIFIED_DESCENDANTS]: "border-l-amber-500/40",
  [ChangeKind.MOVED]: "bg-sky-500/10 border-l-sky-500 text-sky-900 dark:text-sky-200",
};

/** The canvas ring per change, for the overlay. */
export const CHANGE_RING: Record<ChangeKind, string> = {
  [ChangeKind.UNSPECIFIED]: "",
  [ChangeKind.ADDED]: "ring-2 ring-emerald-500",
  [ChangeKind.REMOVED]: "ring-2 ring-rose-500",
  [ChangeKind.MODIFIED]: "ring-2 ring-amber-500",
  [ChangeKind.MODIFIED_DESCENDANTS]: "ring-1 ring-amber-500/40",
  [ChangeKind.MOVED]: "ring-2 ring-sky-500",
};

export const REVIEW_ORDER = [
  ChangeKind.ADDED,
  ChangeKind.MODIFIED,
  ChangeKind.MOVED,
  ChangeKind.REMOVED,
  ChangeKind.MODIFIED_DESCENDANTS,
] as const;

export function displayName(schema: Schema | undefined, kind: string): string {
  return schema?.nodeKinds.find((k) => k.kind === kind)?.displayName ?? shortKind(kind);
}

/** The node kinds `parentKind` may hold as children. */
export function childKinds(schema: Schema | undefined, parentKind: string): string[] {
  return schema?.nodeKinds.find((k) => k.kind === parentKind)?.allowedChildKinds ?? [];
}

/** The edge kinds a user may draw from one node kind to another. */
export function drawableEdgeKinds(
  schema: Schema | undefined,
  fromKind: string,
  toKind: string,
): EdgeKind[] {
  const allows = (list: string[], k: string) => list.length === 0 || list.includes(k);
  return (schema?.edgeKinds ?? []).filter(
    (k) =>
      k.topology !== Topology.TREE && allows(k.fromKinds, fromKind) && allows(k.toKinds, toKind),
  );
}

/** One row of the union of two revision trees, for the tree diff. */
export interface UnionRow {
  key: string;
  label: string;
  kind: string;
  depth: number;
  /** The change, if the node changed. */
  entry?: DiffEntry;
  /** A removed node, or a moved node's old position: shown, not current. */
  ghost: boolean;
  /** The subtree is the same in both revisions. */
  unchanged: boolean;
  /** How many nodes are below it, for collapsed unchanged subtrees. */
  size: number;
}

function sizeOf(n: MerkleNode): number {
  return n.children.reduce((s, c) => s + 1 + sizeOf(c), 0);
}

/**
 * Flatten the target tree in order, merging in the base tree's removed nodes
 * and the old positions of moved nodes as ghosts under their old parents.
 */
export function unionRows(
  base: MerkleNode | undefined,
  target: MerkleNode | undefined,
  entries: DiffEntry[],
): UnionRow[] {
  if (!target) return [];
  const byKey = new Map(entries.map((e) => [e.key, e]));
  const baseIndex = new Map<string, MerkleNode>();
  const baseChildren = new Map<string, MerkleNode[]>();
  const walkBase = (n: MerkleNode) => {
    baseIndex.set(n.key, n);
    baseChildren.set(n.key, n.children);
    n.children.forEach(walkBase);
  };
  if (base) walkBase(base);
  const targetKeys = new Set<string>();
  const walkTarget = (n: MerkleNode) => {
    targetKeys.add(n.key);
    n.children.forEach(walkTarget);
  };
  walkTarget(target);

  const rows: UnionRow[] = [];
  const pushGhosts = (parentKey: string, depth: number) => {
    for (const old of baseChildren.get(parentKey) ?? []) {
      const e = byKey.get(old.key);
      const removed = !targetKeys.has(old.key);
      const movedAway = e?.change === ChangeKind.MOVED && e.fromParent === parentKey;
      if (!removed && !movedAway) continue;
      rows.push({
        key: `${old.key}@${parentKey}`,
        label: old.label,
        kind: old.kind,
        depth,
        entry: removed ? (e ?? undefined) : e,
        ghost: true,
        unchanged: false,
        size: sizeOf(old),
      });
    }
  };
  const walk = (n: MerkleNode, depth: number) => {
    const prev = baseIndex.get(n.key);
    rows.push({
      key: n.key,
      label: n.label,
      kind: n.kind,
      depth,
      entry: byKey.get(n.key),
      ghost: false,
      unchanged: !!prev && prev.subtreeHash === n.subtreeHash,
      size: sizeOf(n),
    });
    for (const c of n.children) walk(c, depth + 1);
    pushGhosts(n.key, depth + 1);
  };
  walk(target, 0);
  return rows;
}

/** A revision's rendered text, cut down to one node's section. */
export function sectionText(
  text: string | undefined,
  sections: { key: string; startLine: number; endLine: number }[] | undefined,
  key: string,
): string | undefined {
  const s = sections?.find((x) => x.key === key);
  if (!text || !s) return undefined;
  return `${text
    .split("\n")
    .slice(s.startLine - 1, s.endLine)
    .join("\n")}\n`;
}

/** The section key that holds a 1-based line. */
export function sectionAt(
  sections: { key: string; startLine: number; endLine: number }[] | undefined,
  line: number,
): string | undefined {
  return sections?.find((s) => line >= s.startLine && line <= s.endLine)?.key;
}

export function shortHash(hash: string): string {
  return hash.slice(0, 8);
}

/** Each node's latest run (highest attempt, then most recently updated). */
export function latestRuns(runs: Run[] | undefined): Map<string, Run> {
  const out = new Map<string, Run>();
  for (const r of runs ?? []) {
    const cur = out.get(r.node);
    const newer =
      !cur ||
      r.attempt > cur.attempt ||
      (r.attempt === cur.attempt && (r.updatedAt?.seconds ?? 0n) > (cur.updatedAt?.seconds ?? 0n));
    if (newer) out.set(r.node, r);
  }
  return out;
}

/** A run's PRs, a group's members in order. */
export function pullRequests(run: Run | undefined): PullRequest[] {
  const prs = (run?.artifacts ?? []).flatMap((a) =>
    a.artifact.case === "pullRequest" ? [a.artifact.value] : [],
  );
  return prs.sort((x, y) => (x.group?.position ?? 0) - (y.group?.position ?? 0));
}

/** The Omnigent session of each of a run's session steps, by step index. */
export function sessions(
  run: Run | undefined,
): { step: number; name: string; session: OmnigentSession }[] {
  return (run?.steps ?? []).flatMap((s) =>
    s.payload && anyIs(s.payload, OmnigentSessionSchema)
      ? [
          {
            step: s.index,
            name: s.name,
            session: anyUnpack(s.payload, OmnigentSessionSchema) as OmnigentSession,
          },
        ]
      : [],
  );
}

/** `#12`, or `#12 1/2` for a member of a PR group. */
export function prLabel(pr: PullRequest): string {
  const position = pr.group?.position ? ` ${pr.group.position}/${pr.group.of || "?"}` : "";
  return `#${pr.number}${position}`;
}

/** An executor stage for display: `in_review` → `in review`. */
export function stageLabel(stage: string): string {
  return stage.replaceAll("_", " ");
}

/** What a node card shows of a run: plain data, no protobuf messages, so it
 * can sit in React Flow's node data. */
export interface RunBadge {
  stage: string;
  costUsd: number;
  prs: { url: string; label: string; title: string; state: string }[];
  /** Set while the run waits for a place: where, and its place in line once
   * the places are known. */
  queued?: { environment: string; position?: number; of?: number };
}

/** Where a waiting run stands in its environment's line, across every
 * graph. `position` is 1-based; it and the counts are unset until the places
 * are known (or if they don't list the run yet). */
export interface QueuePlace {
  environment: string;
  since?: Date;
  /** Its place in line: 1 gets the next free place. */
  position?: number;
  /** How many runs wait in this environment's line. */
  waiting?: number;
  /** How many places are taken, of `max`. */
  held?: number;
  max?: number;
}

/** The place in line of `run`, if it waits for one; from `places`
 * (ListPlaces), which orders the line across every graph. */
export function queuePlace(
  run: Run | undefined,
  places: EnvironmentPlaces[] | undefined,
): QueuePlace | undefined {
  const wait = run?.waiting;
  if (!run || !wait) return undefined;
  const out: QueuePlace = {
    environment: wait.environment,
    since: wait.since ? timestampDate(wait.since as Timestamp) : undefined,
  };
  const env = places?.find((p) => p.environment === wait.environment);
  if (!env) return out;
  const index = env.waiting.findIndex((p) => p.run === run.ref);
  if (index < 0) return out;
  return {
    ...out,
    position: index + 1,
    waiting: env.waiting.length,
    held: env.held.length,
    max: env.maxSessions,
  };
}

/** `1st`, `2nd`, `3rd`, `11th`, `22nd`. */
export function ordinal(n: number): string {
  return `${n}${ORDINAL_SUFFIX[ordinals.select(n)] ?? "th"}`;
}

const ordinals = new Intl.PluralRules("en", { type: "ordinal" });
const ORDINAL_SUFFIX: Partial<Record<Intl.LDMLPluralRule, string>> = {
  one: "st",
  two: "nd", // spellchecker:disable-line
  few: "rd",
};

/** A queued run on its card: `queued · sandbox · 2nd of 3`. */
export function queuedLabel(q: NonNullable<RunBadge["queued"]>): string {
  const place = q.position ? ` · ${ordinal(q.position)} of ${q.of ?? q.position}` : "";
  return `queued · ${q.environment}${place}`;
}

export function runBadge(run: Run | undefined, places?: EnvironmentPlaces[]): RunBadge | undefined {
  if (!run) return undefined;
  const q = queuePlace(run, places);
  return {
    queued: q ? { environment: q.environment, position: q.position, of: q.waiting } : undefined,
    stage: stageLabel(run.stage) || "run",
    costUsd: run.costUsd,
    prs: pullRequests(run).map((pr) => ({
      url: pr.url,
      label: prLabel(pr),
      title: pr.title,
      state: pr.state,
    })),
  };
}

/** `$3.60`. */
export function usd(n: number): string {
  return `$${n.toFixed(2)}`;
}

/** Why a session ran where it did, for display. */
export const PLACEMENT_SOURCE_LABEL: Record<PlacementSource, string> = {
  [PlacementSource.UNSPECIFIED]: "",
  [PlacementSource.RUN]: "chosen for the run",
  [PlacementSource.NODE]: "chosen for the node",
  [PlacementSource.SUBMISSION]: "chosen at submission",
  [PlacementSource.PROFILE]: "profile default",
  [PlacementSource.REGISTRY]: "registry default",
  [PlacementSource.ELIGIBLE]: "first that fits",
  [PlacementSource.DECIDE]: "decided",
};
