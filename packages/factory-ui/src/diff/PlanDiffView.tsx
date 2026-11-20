// The Merkle diff between two revisions of a plan, interactively.
//
// Tree mode: the target tree with the base's removed nodes and moved nodes'
// old positions merged in as ghosts, each row tinted by its change. Unchanged
// subtrees (equal subtree hashes) collapse to one row. Filter by change kind,
// step through changes with j/k (or n/p), toggle compact (review-level)
// entries. Text mode: the two revisions' rendered plans in Monaco's diff
// editor, kept in step with the selection.
//
// The classification always comes from the server's DiffRevisions; this view
// only renders it.

import { ChangeKind, type DiffEntry, type MerkleNode } from "@open-lakehouse/factory-client";
import { Badge, Button, cn, Switch, Tabs, TabsList, TabsTrigger } from "@open-lakehouse/ui-kit";
import { ChevronDown, ChevronUp, Dot, Minus, MoveRight, Pencil, Plus } from "lucide-react";
import { type KeyboardEvent, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { CHANGE_LABEL, CHANGE_TONE, REVIEW_ORDER, type UnionRow, unionRows } from "../model.js";
import { TextDiff } from "../monaco/index.js";
import { useDiff, useRevision } from "../queries.js";

export type DiffMode = "tree" | "text";

export interface PlanDiffViewProps {
  root: string;
  /** The base revision's root hash; empty diffs against nothing. */
  base: string;
  /** The target revision's root hash. */
  target: string;
  selected?: string;
  onSelect?: (key: string) => void;
  mode?: DiffMode;
  onModeChange?: (mode: DiffMode) => void;
  /** Reports the entries shown, e.g. for a canvas overlay. */
  onEntries?: (entries: DiffEntry[]) => void;
  /** The modes offered; both by default. With one, there's no switch. An MCP
   * App's view, which can't load Monaco, offers `["tree"]`. */
  modes?: readonly DiffMode[];
}

const ALL_MODES: readonly DiffMode[] = ["tree", "text"];

const ICON: Record<ChangeKind, typeof Plus> = {
  [ChangeKind.UNSPECIFIED]: Dot,
  [ChangeKind.ADDED]: Plus,
  [ChangeKind.REMOVED]: Minus,
  [ChangeKind.MODIFIED]: Pencil,
  [ChangeKind.MODIFIED_DESCENDANTS]: Dot,
  [ChangeKind.MOVED]: MoveRight,
};

function labels(tree: MerkleNode | undefined, out = new Map<string, string>()) {
  if (!tree) return out;
  out.set(tree.key, tree.label);
  for (const c of tree.children) labels(c, out);
  return out;
}

export function PlanDiffView({
  root,
  base,
  target,
  selected,
  onSelect,
  mode: controlledMode,
  onModeChange,
  onEntries,
  modes = ALL_MODES,
}: PlanDiffViewProps) {
  const [ownMode, setOwnMode] = useState<DiffMode>(modes[0] ?? "tree");
  const wanted = controlledMode ?? ownMode;
  const mode = modes.includes(wanted) ? wanted : (modes[0] ?? "tree");
  const setMode = (m: DiffMode) => (onModeChange ? onModeChange(m) : setOwnMode(m));
  const [compact, setCompact] = useState(true);
  const [showUnchanged, setShowUnchanged] = useState(false);
  const [hidden, setHidden] = useState<Set<ChangeKind>>(new Set());

  const baseRev = useRevision(root, base || undefined);
  const targetRev = useRevision(root, target);
  const diff = useDiff(root, base, target, compact);
  const full = useDiff(root, base, target, false);
  const entries = diff.data ?? [];

  useEffect(() => onEntries?.(full.data ?? []), [full.data, onEntries]);

  const baseTree = base ? baseRev.data?.tree : undefined;
  const targetTree = targetRev.data?.tree;
  const names = useMemo(() => labels(targetTree, labels(baseTree)), [baseTree, targetTree]);

  // Rows come from the full diff so every changed ancestor is marked; the
  // compact switch decides which rows count as changes to step through.
  const rows = useMemo(
    () => unionRows(baseTree, targetTree, full.data ?? []),
    [baseTree, targetTree, full.data],
  );
  const reviewKeys = useMemo(
    () => new Set(entries.filter((e) => !hidden.has(e.change)).map((e) => e.key)),
    [entries, hidden],
  );
  const counts = useMemo(() => {
    const c = new Map<ChangeKind, number>();
    for (const e of entries) c.set(e.change, (c.get(e.change) ?? 0) + 1);
    return c;
  }, [entries]);

  const visible = useMemo(() => {
    const out: (UnionRow & { collapsed?: number })[] = [];
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i];
      if (!showUnchanged && r.unchanged && !r.ghost && r.depth > 0) {
        out.push({ ...r, collapsed: r.size + 1 });
        while (i + 1 < rows.length && rows[i + 1].depth > r.depth) i++;
        continue;
      }
      out.push(r);
    }
    return out;
  }, [rows, showUnchanged]);

  const stops = visible.filter((r) => r.entry && reviewKeys.has(r.entry.key) && !r.collapsed);
  const listRef = useRef<HTMLDivElement>(null);
  const step = (dir: 1 | -1) => {
    if (!stops.length) return;
    const at = stops.findIndex((r) => r.entry?.key === selected);
    const next = stops[(at + dir + stops.length) % stops.length] ?? stops[0];
    if (next.entry) onSelect?.(next.entry.key);
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (["j", "n", "ArrowDown"].includes(e.key)) step(1);
    else if (["k", "p", "ArrowUp"].includes(e.key)) step(-1);
    else return;
    e.preventDefault();
  };
  useEffect(() => {
    if (!selected) return;
    listRef.current
      ?.querySelector(`[data-key="${CSS.escape(selected)}"]`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [selected]);

  const loading = targetRev.isLoading || diff.isLoading || (base && baseRev.isLoading);

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="plan-diff">
      <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2">
        {modes.length > 1 && (
          <Tabs value={mode} onValueChange={(v) => setMode(v as DiffMode)}>
            <TabsList>
              {modes.includes("tree") && <TabsTrigger value="tree">Tree</TabsTrigger>}
              {modes.includes("text") && <TabsTrigger value="text">Text</TabsTrigger>}
            </TabsList>
          </Tabs>
        )}
        <fieldset
          className="m-0 flex flex-wrap items-center gap-1 border-0 p-0"
          aria-label="Filter changes"
        >
          {REVIEW_ORDER.filter((k) => counts.get(k)).map((k) => {
            const Icon = ICON[k];
            const off = hidden.has(k);
            return (
              <button
                key={k}
                type="button"
                aria-pressed={!off}
                onClick={() =>
                  setHidden((h) => {
                    const next = new Set(h);
                    if (off) next.delete(k);
                    else next.add(k);
                    return next;
                  })
                }
                className={cn(
                  "flex items-center gap-1 rounded-full border border-l-4 px-2 py-0.5 text-xs",
                  CHANGE_TONE[k],
                  off && "opacity-40",
                )}
              >
                <Icon className="size-3" />
                {counts.get(k)} {CHANGE_LABEL[k]}
              </button>
            );
          })}
          {!loading && entries.length === 0 ? (
            <span className="text-xs text-muted-foreground">No changes</span>
          ) : null}
        </fieldset>
        <div className="ml-auto flex items-center gap-3 text-xs">
          <span className="flex items-center gap-1.5">
            <Switch checked={compact} onCheckedChange={setCompact} aria-label="Compact" />
            Compact
          </span>
          {mode === "tree" ? (
            <span className="flex items-center gap-1.5">
              <Switch
                checked={showUnchanged}
                onCheckedChange={setShowUnchanged}
                aria-label="Show unchanged"
              />
              Unchanged
            </span>
          ) : null}
          <div className="flex">
            <Button size="sm" variant="ghost" onClick={() => step(-1)} aria-label="Previous change">
              <ChevronUp />
            </Button>
            <Button size="sm" variant="ghost" onClick={() => step(1)} aria-label="Next change">
              <ChevronDown />
            </Button>
          </div>
        </div>
      </div>

      {mode === "tree" ? (
        <div
          ref={listRef}
          role="tree"
          aria-label="Plan diff"
          tabIndex={0}
          onKeyDown={onKeyDown}
          className="min-h-0 flex-1 overflow-auto py-1 font-mono text-[13px] focus:outline-none"
        >
          {loading ? <div className="p-3 text-sm text-muted-foreground">Loading…</div> : null}
          {visible.map((r) => {
            const e = r.entry;
            const change = e?.change ?? ChangeKind.UNSPECIFIED;
            const counted = e && reviewKeys.has(e.key);
            const Icon = ICON[change];
            const nodeKey = r.ghost ? (e?.key ?? r.key.split("@")[0]) : r.key;
            return (
              <div
                key={r.key}
                role="treeitem"
                aria-selected={selected === nodeKey}
                tabIndex={-1}
                data-key={nodeKey}
                data-change={e ? CHANGE_LABEL[change] : "unchanged"}
                onClick={() => onSelect?.(nodeKey)}
                onKeyDown={(ev) => ev.key === "Enter" && onSelect?.(nodeKey)}
                className={cn(
                  "flex cursor-pointer items-center gap-1.5 border-l-4 border-l-transparent py-0.5 pr-3",
                  e &&
                    (counted || change === ChangeKind.MODIFIED_DESCENDANTS) &&
                    CHANGE_TONE[change],
                  e && !counted && change !== ChangeKind.MODIFIED_DESCENDANTS && "opacity-50",
                  r.ghost && "border-dashed italic opacity-60",
                  selected === nodeKey && "bg-accent",
                )}
                style={{ paddingLeft: 8 + r.depth * 16 }}
              >
                <Icon className="size-3.5 shrink-0" aria-hidden />
                <span className="truncate">{r.label}</span>
                <Badge variant="outline" className="px-1 py-0 font-sans text-[10px]">
                  {r.kind}
                </Badge>
                {r.collapsed ? (
                  <span className="font-sans text-xs text-muted-foreground">
                    unchanged{r.collapsed > 1 ? `, ${r.collapsed - 1} below` : ""}
                  </span>
                ) : null}
                {r.ghost ? (
                  <span className="font-sans text-xs text-muted-foreground">was here</span>
                ) : null}
                {!r.ghost && change === ChangeKind.MOVED ? (
                  <span className="font-sans text-xs">
                    from {names.get(e?.fromParent ?? "") ?? e?.fromParent}
                    {e?.contentChanged ? ", and edited" : ""}
                  </span>
                ) : null}
              </div>
            );
          })}
        </div>
      ) : (
        <div className="min-h-0 flex-1">
          <Suspense
            fallback={<div className="p-3 text-sm text-muted-foreground">Loading the editor…</div>}
          >
            <TextDiff
              original={base ? (baseRev.data?.text ?? "") : ""}
              modified={targetRev.data?.text ?? ""}
              originalSections={baseRev.data?.sections}
              modifiedSections={targetRev.data?.sections}
              selected={selected}
              onSelect={onSelect}
            />
          </Suspense>
        </div>
      )}
    </div>
  );
}
