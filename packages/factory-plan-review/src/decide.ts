// The person's decision: approve the reviewed revision, or return the plan
// with notes. Each is an app-only tool call through the host (workgraph-mcp
// lists `approve_revision` and `return_plan` to the desktop's side channel
// only), and its outcome goes to the model with `ui/update-model-context`,
// which the desktop posts into the session as `<mcp-app-context>`.

import type { PlanReview } from "./snapshot.js";

/** What the view needs from its host (ext-apps' `App`). */
export interface ReviewHost {
  callTool(
    name: string,
    args: Record<string, unknown>,
  ): Promise<{
    isError?: boolean;
    content?: { type: string; text?: string }[];
  }>;
  updateModelContext(text: string): Promise<void>;
}

export type Decision =
  | { kind: "approved"; text: string }
  | { kind: "returned"; text: string; notes: string };

export type Outcome = { ok: true; decision: Decision } | { ok: false; error: string };

const name = (review: PlanReview) =>
  review.title ? `the plan "${review.title}" (${review.root})` : `the plan ${review.root}`;

/** What the model is told after an approval. */
export function approvedText(review: PlanReview): string {
  return `Approved revision ${review.revision} of ${name(review)}. Its items may run now.`;
}

/** What the model is told after a return. */
export function returnedText(review: PlanReview, notes: string): string {
  return `Returned ${name(review)} at revision ${review.revision}, unapproved. Revise it and submit it again. The reviewer's notes:\n\n${notes}`;
}

function errorText(result: Awaited<ReturnType<ReviewHost["callTool"]>>): string {
  const text = (result.content ?? [])
    .map((c) => c.text ?? "")
    .join("\n")
    .trim();
  return text || "The factory refused the decision.";
}

async function decide(
  host: ReviewHost,
  tool: string,
  args: Record<string, unknown>,
  decision: Decision,
): Promise<Outcome> {
  let result: Awaited<ReturnType<ReviewHost["callTool"]>>;
  try {
    result = await host.callTool(tool, args);
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
  if (result.isError) return { ok: false, error: errorText(result) };
  await host.updateModelContext(decision.text);
  return { ok: true, decision };
}

export function approve(host: ReviewHost, review: PlanReview): Promise<Outcome> {
  return decide(
    host,
    "approve_revision",
    { root: review.root, revision: review.revision },
    { kind: "approved", text: approvedText(review) },
  );
}

export function returnPlan(host: ReviewHost, review: PlanReview, notes: string): Promise<Outcome> {
  const trimmed = notes.trim();
  if (!trimmed) {
    return Promise.resolve({ ok: false, error: "Say what to change before returning the plan." });
  }
  return decide(
    host,
    "return_plan",
    { root: review.root, revision: review.revision, notes: trimmed },
    { kind: "returned", text: returnedText(review, trimmed), notes: trimmed },
  );
}
