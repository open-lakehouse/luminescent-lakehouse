// The plan review: the plan's work graph (factory-ui's PlanCanvas, tinted by
// what changed) and its Merkle diff against the approved revision
// (PlanDiffView, tree mode), with the person's decision underneath. Everything
// it shows comes from the tool result's snapshot (snapshot.ts).

import {
  FactoryClientProvider,
  PlanCanvas,
  PlanDiffView,
  usePayloadRegistry,
} from "@open-lakehouse/factory-ui";
import {
  Badge,
  Button,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Textarea,
} from "@open-lakehouse/ui-kit";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { approve, type Decision, type ReviewHost, returnPlan } from "./decide.js";
import { decodeSnapshot, type PlanReview, type Snapshot, snapshotClient } from "./snapshot.js";

const short = (hash: string) => hash.slice(0, 12);

const CHANGE_ORDER = ["added", "removed", "modified", "moved", "modified_descendants"];
const CHANGE_LABEL: Record<string, string> = {
  added: "added",
  removed: "removed",
  modified: "modified",
  moved: "moved",
  modified_descendants: "changed below",
};

export interface PlanReviewViewProps {
  review: PlanReview;
  host: ReviewHost;
  colorMode?: "light" | "dark";
}

export function PlanReviewView({ review, host, colorMode }: PlanReviewViewProps) {
  const snapshot = useMemo(() => decodeSnapshot(review.snapshot), [review]);
  const client = useMemo(() => snapshotClient(review, snapshot), [review, snapshot]);
  const queryClient = useMemo(
    () =>
      new QueryClient({
        defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
      }),
    [],
  );
  return (
    <QueryClientProvider client={queryClient}>
      <FactoryClientProvider client={client}>
        <Review review={review} snapshot={snapshot} host={host} colorMode={colorMode} />
      </FactoryClientProvider>
    </QueryClientProvider>
  );
}

function Review({
  review,
  snapshot,
  host,
  colorMode,
}: PlanReviewViewProps & { snapshot: Snapshot }) {
  const registry = usePayloadRegistry();
  const [selected, setSelected] = useState<string>();
  const [notes, setNotes] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [decision, setDecision] = useState<Decision>();
  const changes = useMemo(
    () => new Map(snapshot.diff.entries.map((e) => [e.key, e])),
    [snapshot.diff.entries],
  );
  const counts = CHANGE_ORDER.filter((k) => (review.changes[k] ?? 0) > 0);
  const approved = review.approved_revision;

  const run = async (decide: () => ReturnType<typeof approve>) => {
    setBusy(true);
    setError(undefined);
    const outcome = await decide();
    setBusy(false);
    if (outcome.ok) setDecision(outcome.decision);
    else setError(outcome.error);
  };

  return (
    <main className="flex flex-col gap-3 p-3 text-sm" aria-label="Plan review">
      <header className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h1 className="font-semibold text-base">{review.title || review.root}</h1>
        <span className="font-mono text-muted-foreground text-xs" title={review.revision}>
          revision {short(review.revision)}
        </span>
        <span className="text-muted-foreground text-xs">
          {approved === null
            ? "never approved"
            : approved === review.revision
              ? "unchanged since its approval"
              : `since approved ${short(approved)}`}
        </span>
        <span className="flex flex-wrap gap-1" data-testid="review-changes">
          {counts.map((k) => (
            <Badge key={k} variant="outline">
              {review.changes[k]} {CHANGE_LABEL[k] ?? k}
            </Badge>
          ))}
        </span>
      </header>

      <Tabs defaultValue="changes">
        <TabsList>
          <TabsTrigger value="changes">Changes</TabsTrigger>
          <TabsTrigger value="plan">Plan</TabsTrigger>
        </TabsList>
        <TabsContent value="changes" className="max-h-[420px] overflow-auto">
          <PlanDiffView
            root={review.root}
            base={approved ?? ""}
            target={review.revision}
            selected={selected}
            onSelect={setSelected}
            modes={["tree"]}
          />
        </TabsContent>
        <TabsContent value="plan">
          <div className="h-[420px] rounded-md border" data-testid="plan-canvas">
            <PlanCanvas
              graph={snapshot.graph.graph}
              schema={snapshot.schema.schema}
              registry={registry}
              selected={selected}
              onSelect={setSelected}
              changes={changes}
              readOnly
              colorMode={colorMode}
            />
          </div>
        </TabsContent>
      </Tabs>

      {decision ? (
        <p role="status" className="rounded-md border bg-muted p-3">
          {decision.kind === "approved"
            ? `Approved revision ${short(review.revision)}. The planner has been told.`
            : "Returned to the planner with your notes."}
        </p>
      ) : (
        <section className="flex flex-col gap-2" aria-label="Decision">
          <Textarea
            aria-label="Notes for the planner"
            placeholder="What should change? (needed to return the plan)"
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            disabled={busy}
            rows={3}
          />
          <div className="flex gap-2">
            <Button disabled={busy} onClick={() => run(() => approve(host, review))}>
              Approve revision {short(review.revision)}
            </Button>
            <Button
              variant="outline"
              disabled={busy || !notes.trim()}
              onClick={() => run(() => returnPlan(host, review, notes))}
            >
              Return with notes
            </Button>
          </div>
          {error && (
            <p role="alert" className="text-destructive">
              {error}
            </p>
          )}
        </section>
      )}
    </main>
  );
}
