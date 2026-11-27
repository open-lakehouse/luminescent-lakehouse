// A work-graph node on the canvas: kind, title, phase and derived conditions,
// chips for its payload (a gate's condition, a task's extensions), its latest
// run (stage, cost, PRs with their place in a group), with handles for the
// hierarchy (top/bottom) and for drawing edges (left/right), and a menu to
// add a child of an allowed kind.

import { type ChangeKind, type Node, Phase } from "@open-lakehouse/factory-client";
import {
  Badge,
  Button,
  cn,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@open-lakehouse/ui-kit";
import { Handle, type NodeProps, Position, type Node as RFNode } from "@xyflow/react";
import { ExternalLink, Plus } from "lucide-react";
import {
  CHANGE_LABEL,
  CHANGE_RING,
  PHASE_LABEL,
  queuedLabel,
  type RunBadge,
  usd,
} from "../model.js";
import { NODE_SIZE } from "./layout.js";

export interface WorkNodeData extends Record<string, unknown> {
  node: Node;
  kindName: string;
  /** One line each: a gate's condition, a task's extensions. */
  chips?: string[];
  childKinds: { kind: string; name: string }[];
  change?: ChangeKind;
  /** The node's latest run, if it has one. */
  run?: RunBadge;
  dropTarget?: boolean;
  readOnly?: boolean;
  onAddChild?: (kind: string) => void;
}

export type WorkFlowNode = RFNode<WorkNodeData, "work">;

const CONDITION_TONE: Record<string, string> = {
  Ready: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
  Blocked: "bg-rose-500/15 text-rose-700 dark:text-rose-300",
  Running: "bg-sky-500/15 text-sky-700 dark:text-sky-300",
  Stalled: "bg-orange-500/15 text-orange-700 dark:text-orange-300",
  Drifted: "bg-amber-500/15 text-amber-700 dark:text-amber-300",
  InReview: "bg-violet-500/15 text-violet-700 dark:text-violet-300",
  AwaitingGate: "bg-slate-500/15 text-slate-700 dark:text-slate-300",
};

const PR_TONE: Record<string, string> = {
  open: "border-emerald-500/40",
  merged: "border-violet-500/40 opacity-70",
  closed: "border-rose-500/40 line-through opacity-60",
};

const PHASE_TONE: Record<Phase, string> = {
  [Phase.UNSPECIFIED]: "",
  [Phase.PROPOSED]: "border-dashed",
  [Phase.APPROVED]: "",
  [Phase.DONE]: "opacity-70",
  [Phase.DROPPED]: "opacity-50 line-through",
};

export function WorkNode({ data, selected }: NodeProps<WorkFlowNode>) {
  const { node, kindName, chips, childKinds, change, run, dropTarget, readOnly, onAddChild } = data;
  const phase = node.status?.phase ?? Phase.UNSPECIFIED;
  const conditions = (node.status?.conditions ?? []).filter((c) => c.status);
  return (
    <div
      className={cn(
        "flex flex-col gap-1.5 rounded-lg border bg-card p-2.5 text-card-foreground shadow-sm transition-shadow",
        PHASE_TONE[phase],
        selected && "border-primary shadow-md",
        dropTarget && "ring-2 ring-primary ring-offset-2",
        change !== undefined && CHANGE_RING[change],
      )}
      style={{ width: NODE_SIZE.width, height: NODE_SIZE.height }}
      data-testid={`work-node-${node.ref}`}
      data-change={change !== undefined ? CHANGE_LABEL[change] : undefined}
    >
      <Handle
        type="target"
        position={Position.Top}
        id="parent"
        className="!bg-muted-foreground/40"
        isConnectable={false}
      />
      <Handle
        type="target"
        position={Position.Left}
        id="in"
        className="!h-3 !w-1.5 !rounded-sm !bg-primary/60"
      />
      <div className="flex items-center gap-1.5">
        <Badge variant="outline" className="px-1.5 py-0 text-[10px] uppercase tracking-wide">
          {kindName}
        </Badge>
        <span className="text-[11px] text-muted-foreground">{PHASE_LABEL[phase]}</span>
        <div className="ml-auto flex items-center gap-0.5">
          {node.metadata?.externalUrl ? (
            <a
              href={node.metadata.externalUrl}
              target="_blank"
              rel="noreferrer"
              className="text-muted-foreground hover:text-foreground"
              aria-label="Open in its backend"
            >
              <ExternalLink className="size-3.5" />
            </a>
          ) : null}
          {!readOnly && childKinds.length > 0 ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="nodrag size-6"
                  aria-label="Add a child"
                >
                  <Plus className="size-3.5" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuLabel>Add a child</DropdownMenuLabel>
                {childKinds.map((k) => (
                  <DropdownMenuItem key={k.kind} onSelect={() => onAddChild?.(k.kind)}>
                    {k.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
        </div>
      </div>
      <div
        className={cn("text-sm font-medium leading-snug", run ? "line-clamp-1" : "line-clamp-2")}
        title={node.spec?.title}
      >
        {node.spec?.title}
      </div>
      {run ? (
        <div
          className="flex items-center gap-1 text-[10px] text-muted-foreground"
          data-testid="run"
        >
          {run.queued ? (
            <span
              className="font-medium"
              data-testid="run-queued"
              title={`Waiting for a place in ${run.queued.environment}`}
            >
              {queuedLabel(run.queued)}
            </span>
          ) : (
            <span className="font-medium">{run.stage}</span>
          )}
          {run.costUsd ? <span>{usd(run.costUsd)}</span> : null}
          <span className="ml-auto flex gap-0.5">
            {run.prs.map((pr) => (
              <a
                key={pr.url}
                href={pr.url}
                target="_blank"
                rel="noreferrer"
                title={`${pr.title} (${pr.state})`}
                className={cn(
                  "nodrag rounded border px-1 hover:text-foreground",
                  PR_TONE[pr.state],
                )}
              >
                {pr.label}
              </a>
            ))}
          </span>
        </div>
      ) : null}
      <div className="mt-auto flex flex-wrap gap-1">
        {(chips ?? []).map((chip) => (
          <span
            key={chip}
            title={chip}
            data-testid="payload-chip"
            className="max-w-full truncate rounded border px-1.5 py-px text-[10px]"
          >
            {chip}
          </span>
        ))}
        {conditions.map((c) => (
          <span
            key={c.type}
            title={c.message || c.reason}
            className={cn("rounded px-1.5 py-px text-[10px] font-medium", CONDITION_TONE[c.type])}
          >
            {c.type}
          </span>
        ))}
        {change !== undefined ? (
          <span className="ml-auto rounded bg-muted px-1.5 py-px text-[10px]">
            {CHANGE_LABEL[change]}
          </span>
        ) : null}
      </div>
      <Handle
        type="source"
        position={Position.Right}
        id="out"
        className="!h-3 !w-1.5 !rounded-sm !bg-primary/60"
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="child"
        className="!bg-muted-foreground/40"
        isConnectable={false}
      />
    </div>
  );
}
